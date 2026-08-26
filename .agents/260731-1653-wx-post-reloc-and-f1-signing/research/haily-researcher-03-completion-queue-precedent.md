---
name: completion-queue-precedent
description: NET_RX migration precedent (commits 2c2c81e2, 49a15348) and CompletionQueue API surface, mined for what transfers to a Recv migration
---

# Completion Queue Precedent — Mined from Git History

## CompletionQueue full API — one queue per cell, all sources share 32 slots
IPC-recv reservations and NET_RX reservations from the same cell compete for the same 32-slot ring — no per-source sub-allocation.
- `QUEUE_CAPACITY = 32` (`completion.rs:45`); `queue_for(sched, tid)` (`completion.rs:315`) finds-or-creates one `Arc<CompletionQueue>` per `cell_id`, cached on every task's `completion` field.
- API: `reserve() -> Option<SlotId>` (139), `complete(slot, result: isize) -> bool` `#[must_use]` (159), `release(slot) -> bool` (214, added in `49a15348` — refuses if slot already `Done`, forcing `drain()` instead of discard), `drain() -> Option<Completion>` FIFO (229), `register_waiter`/`clear_waiter` (285/290, **single waiter per queue**), `reserved()`/`drainable()` (266/272).
- Locking: every op takes one `Spinlock<Ring>` leaf lock; `Spinlock::lock()` disables interrupts before spinning, so `reserve/complete/release/drain` are interrupt-context-safe. `queue_for` requires caller already holds `SCHEDULER`.
- Wakes deferred, never inline: `complete()` only flags `WAKES_PENDING`; `deliver_pending_wakes(sched)` (`completion.rs:352`) does the actual `Ready` transition + `push_ready`, called from `yield_cpu()`.

## completion_wait.rs is hardcoded to NET_RX — cannot be reused as-is for IPC recv
- `wait_completion()` checks `if mask != api::syscall::events::NET_RX { return Err(InvalidInput) }` — literal equality, not bit-membership.
- `validate_user_buf(out_ptr, COMPLETION_LEN, COMPLETION_LEN)` pins both len and max to 24 bytes.
- `ViCompletion` (`libs/api/src/abi/completion.rs`, `COMPLETION_LEN = 24`): `[0..4] magic, [4..8] version, [8..12] slot:u32, [12..16] reserved (4 spare bytes), [16..24] result:i64]`.
- `libs/api/src/abi.rs` marks this tree **FROZEN ABI**: changes require 2x explicit user confirmation.
- **Resolution adopted in solution design:** IPC recv does NOT need `ViCompletion` extended or a new syscall. `result: i64` carries a byte count under the new "drain your mailbox now" contract; sender identity comes from the drained `PendingMsg`, not the completion record. The 4 spare bytes stay spare. `completion_wait.rs`'s `mask` guard stays untouched in stage 1 — the receiver parks inside `ipc_recv` itself and drains on resume, it does not go through `wait_completion()` at all in the first cut. Multiplexing IPC recv into the same user-facing `WaitCompletion` syscall (adding an `IPC_RECV` mask bit) is a stage-2 follow-up, not required for this migration to be correct.

## arm_net_rx pattern — no interrupt for IPC recv; "arming" happens inside ipc_send itself
- `arm_net_rx(queue, slot)` (`waker.rs:69`) runs from the waiting cell's own syscall context, stores `(queue, slot)` in one global `Spinlock<NetRxWait>`, completing any previously-armed reservation as `RESULT_ABANDONED`.
- `signal_net_rx()` (`waker.rs:120`, ISR context) completes the armed reservation if present, else sets level flag `NET_RX_PENDING`.
- **IPC recv has no ISR and needs no separate wait table.** `ipc_send`/`ipc_post_nonblock`/`ipc_try_send` already hold the target `Task` under `SCHEDULER` and inspect its state directly (`task.rs:1215` etc.) — the reservation stored *in the parked task's own `Recv` variant* (`slot: Option<SlotId>` field) **is** the wait table. No global singleton, no per-cell registry to build from scratch.

## tcb.rs — queue is per-cell, lazily created by whichever task calls first
- Field: `pub completion: Option<Arc<CompletionQueue>>` (`tcb.rs`, init `None` in `Task::new()`).
- `queue_for(sched, tid)`: checks the calling task's own field first, else scans all tasks sharing `cell_id` for an existing queue before allocating — genuinely per-cell, shared across every task of that cell, zero heap cost until first use.

## Hard blocker carried into solution design: one waiter per queue
`register_waiter` supports exactly one tid per queue (`completion.rs:285`). NET_RX gets away with this by convention (one receiver per cell). Two tasks of one cell simultaneously in `Recv` is representable today and would collide under a naive migration. **Resolution:** `slot: Option<SlotId>` on `TaskState::Recv` — if `reserve()` or the waiter seat is already taken by a different task, the second `Recv` simply gets `slot: None` and follows the legacy (pre-migration) park/wake path. Never an error, never a correctness change — purely a "does this particular wait get the fast completion-queue wake, or the old scheduler-sweep wake" choice.

## Environment note
This research was conducted via `git show <sha>` history reconstruction in an isolated worktree that did not have `feat/wx-post-reloc-and-f1-signing`'s commits checked out live; content verified against the diffs of `2c2c81e2` and `49a15348`, not a live build. Recommend confirming exact current line numbers against the actual working tree before implementation (git history is source-of-truth for the *pattern*, not necessarily exact current line numbers after subsequent commits).
