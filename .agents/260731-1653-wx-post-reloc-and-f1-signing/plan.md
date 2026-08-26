---
title: "Fix the Recv buffer-pinning hazard via pending_msgs (completion queue deferred)"
description: "Close the ADR-flagged unsafe direct-buffer-write hazard in IPC Recv delivery by routing it through the existing pending_msgs mailbox. Does NOT migrate Recv's wait mechanism onto the completion queue — that was attempted, red-teamed, found fatally flawed (slot-lifecycle and queue-sharing issues), and is deferred."
status: completed
priority: P1
effort: 4 phases
branch: feat/wx-post-reloc-and-f1-signing
tags: [kernel, ipc, scheduler, adr-03b, memory-safety]
created: 2026-07-31
---

# Fix the Recv Buffer-Pinning Hazard (Completion Queue Deferred)

Governed by `docs/specs/03b-async-reactor-adr.md` (accepted 2026-07-31). Full research:
[scout-report.md](scout-report.md) and `research/`.

## Revision history

**First draft** (superseded): migrate `TaskState::Recv`'s wait mechanism onto the per-cell
completion queue, mirroring NET_RX (`49a15348`), via an additive `slot: Option<SlotId>` field.
Red-teamed and **blocked**: slots are never freed on the success path (every delivered message
permanently burns one of the cell's 32 shared slots), the most common Recv teardown (`ipc_reply`)
wasn't covered, `register_waiter` doesn't have the arbitration semantics the design assumed, and
the shared 32-slot queue creates a NET_RX-starvation path from ordinary IPC traffic. See
[research/red-team-findings.md](research/red-team-findings.md) for the full review.

**This revision**: fix only the memory-safety hazard the ADR actually requires be audited before
any executor change — the three sites where the kernel writes into another task's raw buffer from
foreign context — without touching the completion queue at all. This is smaller, correct, and
unblocks the real, present risk (a use-after-free-shaped hazard) without inheriting the
first draft's slot-lifecycle problems. Migrating Recv's wait mechanism onto the completion queue,
if wanted later, is a separate future effort scoped independently.

## Phases

| Phase | Title | Status | Risk | Depends on |
|-------|-------|--------|------|------------|
| [01](phase-01-drain-on-wake-infra.md) | Resume-side pending_msgs drain-on-wake (infrastructure, no producer change) | completed | low | — |
| [02](phase-02-route-delivery-pending-msgs.md) | Route the 3 delivery sites through pending_msgs | completed | medium | 01 |
| [03](phase-03-test-matrix.md) | Full test matrix | completed | medium | 02 |
| [04](phase-04-guardrails-docs.md) | Documentation guardrails for deferred items | completed | low | 02 |

Phase 04 can run in parallel with Phase 03 once Phase 02 lands.

## Key design decision

`TaskState::Recv` is unchanged — no new field, no new variant. Only the *action taken on match*
changes in three functions (`ipc_send`, `ipc_post_nonblock`, `ipc_try_send`): instead of an unsafe
direct copy into the target's `buf_ptr`, they push an owned message into the target's existing
`pending_msgs` mailbox and wake it exactly as today (`push_ready` + `pend_preempt_if_needed`,
unchanged). The receiver, on resume, drains its own mailbox into its own buffer in its own
context — the same "stash owned data, let the resumed task copy it itself" pattern already used
by `pending_exit_reason` and the hotswap `PendingMsg` fallback path. This requires one genuinely
new piece: today `pending_msgs` is only drained *before* a task parks (self-poll); it has never
been drained *after* a wake, because nothing has ever delivered into a parked `Recv` task's
mailbox before. Phase 01 adds that missing drain-on-wake step.

## Why not the completion queue

The red-teamed first draft is the record of why: reusing NET_RX's shared 32-slot per-cell queue
for IPC recv creates a resource-exhaustion path (ordinary request/reply IPC starves the NIC),
`register_waiter`'s real semantics don't provide the seat-arbitration a rendezvous primitive with
possibly-multiple-parked-tasks needs, and the queue's variant-embedded bookkeeping is destroyed by
every one of Recv's several teardown paths (`ipc_reply`, `NotifyOnExit` wake, hotswap freeze).
Making that work correctly would mean giving IPC recv its own slot space independent of NET_RX's,
tracking that bookkeeping on the `Task` struct instead of inside `TaskState::Recv`, and auditing
every teardown path — a materially larger change than what's needed to close the actual
memory-safety hazard. Deferred, not abandoned; tracked in Phase 04.

## Explicitly out of scope (documented in Phase 04, not fixed here)

- Migrating Recv's wait mechanism onto the completion queue (see above).
- Reply-waiter-not-woken-on-peer-exit hole (pre-existing, orthogonal — `exit_task` only wakes
  `Sending`-matched peers and `NotifyOnExit` watchers, not a plain reply-waiter; unaffected by this
  change either way since no queue slot is at stake).
- `ipc_post_nonblock` ignored-mask bug (latent, unrelated to transport).
- `fat.rs` dead-code `read_async` future (needs a comment, not a fix).
- `ipc_borrow_write`/`ipc_borrow_read` (no live caller).
- `Sending`-side reads (`ipc_recv`/`ipc_try_recv` reading from a sender's `msg_ptr`) — unaffected,
  out of scope.

## Success criteria (whole plan)

- The three delivery-side unsafe sites (`ipc_send`, `ipc_post_nonblock`, `ipc_try_send`) no longer
  write into a foreign task's `buf_ptr` from outside its own execution context.
- Shell keyboard input (integration test) and the input-event burst case both still pass, at the
  existing depth, with no new spurious `TryAgain`.
- `ipc_reply`-based request/reply round trips (the most common Recv teardown, e.g. VFS calls) are
  unaffected.
- `RecvScatter` is explicitly tested, not assumed unchanged, given its kernel-heap temp-buffer
  shape flagged during review.
- `docs/specs/03b-async-reactor-adr.md` gets a short Consequences note recording that the
  buffer-pinning audit item is addressed, and that the completion-queue migration itself remains
  future work.
