---
name: scout-report
description: Codebase analysis + solution design synthesis for the Recv-to-completion-queue migration (superseded design — see note)
---

> **Superseded.** The "Solution design" section below (additive `slot` field on `TaskState::Recv`)
> was red-teamed and blocked — see [research/red-team-findings.md](research/red-team-findings.md).
> The plan now in [plan.md](plan.md) fixes only the buffer-pinning hazard via `pending_msgs`, with
> no completion-queue involvement. The research below (state machine, buffer-pinning audit,
> completion-queue precedent) is still accurate and still the basis for the revised plan — only
> the "Solution design" section's conclusion no longer applies.

# Scout Report — Migrate Recv onto the Completion Queue

## Governing constraint

`docs/specs/03b-async-reactor-adr.md` (accepted, same-day as this plan) requires that the rendezvous receive path and non-blocking send move together — leaving one aware of a parked-in-completion-queue state and the other not causes silent message-delivery failure (the ADR names the shell's keyboard input path by name).

## Research (see `research/` for full detail)

1. `haily-researcher-01-recv-send-state-machine.md` — full `ipc_send`/`ipc_recv`/`ipc_try_recv`/`TryRecv`/`RecvTimeout` state machine, every `TaskState::Recv`/`Sending` call site (15 hits, all in `kernel/src/task.rs`, `scheduler.rs`, `hotswap.rs`), the `pending_msgs` mailbox, task-exit cleanup, and the shell/keyboard input path.
2. `haily-researcher-02-buffer-pinning-audit.md` — the ADR-mandated audit: five sites write into/read from another task's raw pointer on the assumption "it's parked, nothing else can run." Three are delivery-side (in scope), two are `Sending`-side reads (out of scope, unaffected by this migration).
3. `haily-researcher-03-completion-queue-precedent.md` — `CompletionQueue` API, the NET_RX migration's exact pattern, and why it does not transfer verbatim (ABI-frozen 24-byte completion record, one-waiter-per-queue, no ISR to arm against).

## Solution design — two-lens synthesis

A risk-first and a simplicity-first review independently converged on the same design:

**Do not invent a new `TaskState` variant or a new syscall.** Add `slot: Option<SlotId>` to the existing `TaskState::Recv` variant. This is additive and reversible: `None` behaves exactly as today. It also means `ipc_post_nonblock`/`ipc_try_send`/`ipc_send`'s match against `TaskState::Recv` never changes — the ADR's "leaving one aware and the other not" hazard is avoided structurally, without touching `Sending` at all, because the receiver is represented by the same state either way.

**Route delivery through the existing `pending_msgs` mailbox instead of a direct buffer write.** This is the fix for the buffer-pinning hazard: the three delivery-side unsafe sites (`ipc_send:1227-1233`, `ipc_post_nonblock:1291-1297`, `ipc_try_send:1478-1484`) stop writing into the target's `buf_ptr` from foreign context; they push an owned message (mirroring the existing hotswap `PendingMsg` shape) and either `complete(slot, len)` (fast path) or `push_ready` directly (fallback path, `slot: None`). The receiver, on resume, drains its own mailbox into its own buffer in its own syscall context — reusing the drain-before-park logic the `Recv`/`RecvTimeout`/`TryRecv` syscall handlers already have for the Frozen/busy fallback case.

**No ABI change.** `ViCompletion.result: i64` carries a byte count under this contract; sender identity comes from the drained message, not the completion record. The frozen 24-byte layout is untouched, and `completion_wait.rs`'s `WaitCompletion` syscall is untouched in this migration — stage 1 buys the pinning fix and the plumbing, not user-visible multiplexing. A future stage could add an `IPC_RECV` mask bit to `WaitCompletion` itself; not required for correctness here.

**One-waiter-per-queue is handled as a fallback, not an error.** If `reserve()` or the single waiter seat is unavailable (e.g., a second task in the same cell already parked in `Recv`), the new `Recv` simply gets `slot: None` and takes the legacy park/wake path. Never `EAGAIN`, never a behavior change the caller can observe beyond which internal wake mechanism fired.

**Every non-delivery exit from a reserved `Recv` must release the slot.** The RecvTimeout deadline sweep (`scheduler.rs:653-658`) and `exit_task` must both call `release()` when a slot-bearing `Recv` is torn down — the queue is shared with NET_RX on that cell, and a leaked slot permanently steals NET_RX capacity. This is the single highest-severity correctness risk identified in review.

## Explicitly deferred (documented, not fixed, in this change)

- The pre-existing hole where a plain reply-waiter (`Recv{mask:target}` after `ipc_send`) is not woken by `exit_task` unless it separately called `NotifyOnExit` — orthogonal to the transport this migration changes, and fixing it means touching exit teardown semantics, a second independent failure surface. Document with a code comment; do not fix here.
- `ipc_post_nonblock`'s ignored-mask bug (`task.rs:1284-1289`) — latent, unrelated to the transport, fix as its own follow-up commit.
- `fat.rs:467-489` `read_async` — dead code, never wired to a live syscall; add a "do not revive without a cancel/unpin point" comment only.
- `ipc_borrow_write`/`ipc_borrow_read` (`task.rs:1548-1605`) — no live caller outside tests, out of scope.
- Migrating `Sending` itself, or `RecvScatter`/`SendGather` beyond their existing thin-wrapper behavior — not required by the design above.
- Multiplexing IPC recv into the public `WaitCompletion` syscall — stage 2, not required for stage 1 correctness.

## Open verification items carried into implementation (see phase Assumptions sections)

- Whether `pending_msgs` is one Vec bounds-checked against different depth constants per producer, or two distinct fields (`HOTSWAP_MSG_QUEUE_DEPTH=64` vs `INPUT_EVENT_QUEUE_DEPTH=512`) — must preserve each producer's existing depth, not accidentally regress the 512-deep input queue to 64.
- Exact current line numbers for the two `exit_task` call sites not individually read (`task.rs:873`, `syscall.rs:2608`).
