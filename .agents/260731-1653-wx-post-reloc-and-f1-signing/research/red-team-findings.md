---
name: red-team-findings
description: Adversarial review that blocked the original completion-queue design for the Recv migration
metadata:
  type: reference
---

# Red Team Findings — Original Completion-Queue Design (BLOCKED)

Reviewed the first draft of this plan (slot-based migration of `TaskState::Recv` onto the
per-cell completion queue). Verdict: **BLOCKED**. Reproduced here in full since it is the record
of why the plan changed direction.

**VERDICT:** BLOCKED — the central mechanism does not exist as the plan described it: the
resume-side drain it depended on was absent, no path ever returned a completed slot to `Free`,
and `register_waiter` has no seat arbitration, so the design as written produced the exact silent
discard the ADR forbids, plus a 32-message-per-cell resource cliff.

## Critical findings

- **`kernel/src/task/syscall.rs:1385-1412` (Recv) / `:1584-1592` (RecvTimeout)** — the
  `pending_msgs` drain runs only *before* `ipc_recv`; after `yield_cpu()` the handler reads
  `t.current_caller` and returns it with no drain and no copy. Routing delivery through
  `pending_msgs` as originally designed would wake the receiver with an unwritten buffer.
- **`kernel/src/task/completion.rs:214-223`** — no path returns a delivered slot to `Free`.
  `complete()` moves `Reserved → Done(result)`; only `drain()` frees it, and `release()` on a
  `Done` slot is a no-op. The original design explicitly never drained the completion record
  (payload lived in `pending_msgs` instead), so every delivered message would permanently burn one
  of `QUEUE_CAPACITY = 32` slots. After 32 messages the cell's `reserve()` fails forever.
- **`kernel/src/task.rs:1534` (`ipc_reply`)** — the most common Recv teardown (every VFS
  request/reply pair) sets `t.state = TaskState::Ready` directly; the reservation would stay
  `Reserved` forever. Not in the original release-path enumeration.
- **`kernel/src/task/completion.rs:373-381`** — `deliver_pending_wakes` sets
  `task.state = TaskState::Ready` for any parked task, discarding the whole `Recv` variant
  (including an embedded `slot` field). There would be no way for the resumed receiver to learn
  which slot to drain.
- **`kernel/src/task/completion.rs:285-287,295-303`** — `register_waiter(&self, tid: usize)`
  returns `()`, not `bool`. The original design's "fall back if the seat is taken" logic doesn't
  compile and the safety property it assumed doesn't exist: a second `Recv` in the same cell would
  silently steal the wake, and the first task would hang.

## High-severity findings

- `NotifyOnExit` watcher wake (`scheduler.rs:551-561`) and hotswap freeze
  (`hotswap.rs:142`) are two more slot-dropping teardowns the original design missed.
- `release()` is `#[must_use]`, returns `false` on an already-`Done` slot — original
  release-on-timeout/exit pseudocode ignored the return, silently leaking on a lost race.
- The reply-waiter-not-covered-by-`exit_task` hole is not orthogonal under a slot-based design —
  it converts a single hung task into cell-wide `WaitCompletion`/NET_RX starvation, since the hung
  task also holds a reservation forever.
- `pending_msgs` depth resolves to one `Vec` field with two different bounds (64 vs 512) used by
  different producers — mirroring the wrong constant for normal `ipc_send` traffic would introduce
  new `TryAgain` failures that don't exist today.
- Preserving `ipc_post_nonblock`'s ignored-mask behavior "verbatim" is incompatible with a
  mask-filtered mailbox drain under the original slot-based wake design.
- `kernel/src/memory/heap.rs` — the owned copy under `pending_msgs` is charged to the sender's
  cell quota; a chatty client crossing its quota under allocation could hit `alloc_error_handler`
  and hang rather than error gracefully. **Still relevant to the revised (no-queue) design — carry
  into Phase 02/03.**

## Medium-severity findings

- Delivery via `complete()` defers the wake to `yield_cpu()`, losing the RT preemption that
  `push_ready` + `pend_preempt_if_needed` provide today — **not applicable to the revised design**,
  which keeps the existing direct wake mechanism unchanged.
- `deliver_pending_wakes` skips `Frozen` tasks — a completion for a receiver frozen mid-hotswap
  would be a lost wakeup with no retry. **Not applicable to the revised design** (no completion
  queue involved).
- `RecvScatter` (`syscall.rs:1495-1497`) passes a kernel-heap `tmp` buffer as `buf_ptr` and
  reportedly never yields through the normal park path — the buffer-pinning audit missed this as a
  distinct victim-side site. **Still relevant** — carried into the revised plan as an explicit
  investigation item (Phase 01 assumption, Phase 03 test case).
- Lock-ordering documentation would need updating if delivery sites called `complete()` while
  holding `SCHEDULER`. **Not applicable to the revised design.**
- Registering a `Recv` task as the queue's waiter is itself a behavior change (wakes on any
  completion, not just its own). **Not applicable to the revised design.**

## Positive findings (retained)

- The buffer-pinning audit itself (which unsafe sites are real, that `Sending`'s sender-side reads
  should stay untouched) held up under adversarial review — the flaws were in the completion-queue
  wiring, not the underlying problem diagnosis. This audit is what the revised plan still acts on.
- The underlying `CompletionQueue`'s own state machine (reserve/complete/release/drain,
  `#[must_use]` on fallible transitions) is precise enough that the original plan's gaps were
  mechanically detectable — the API itself does not need changing; it simply isn't the right tool
  for a synchronous rendezvous primitive with multiple teardown paths.

## What carries forward into the revised (no-queue) plan

- The three delivery-side unsafe sites and the stash-then-self-write fix shape — unchanged,
  still the core of the fix.
- `pending_msgs` depth-semantics verification — still required.
- Heap-quota/allocation-failure risk under the sender-charged owned copy — still required.
- `RecvScatter`'s kernel-heap temp-buffer shape — still an open investigation item.
- `ipc_reply`'s delivery mechanism — now only relevant to confirm it routes through `ipc_send`
  (and therefore inherits Phase 02's fix for free) rather than duplicating its own direct-write
  logic; no longer a slot-leak concern since no slot exists.
