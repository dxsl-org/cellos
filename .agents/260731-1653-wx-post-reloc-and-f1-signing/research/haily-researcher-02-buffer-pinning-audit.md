---
name: buffer-pinning-audit
description: ADR-mandated audit of unsafe code justified by "the caller is blocked" — every site that would be invalidated by a cancellable completion-queue park
---

# Buffer-Pinning Audit — ADR-Mandated, Pre-Migration

The ADR (`docs/specs/03b-async-reactor-adr.md`, Consequences) requires this audit happen *before* the executor changes, not after: "Every block of unsafe code justified by 'the caller is blocked' must be audited... That justification stops being true the moment a future can be cancelled."

## The ADR-named filesystem case: fat.rs `read_async`, dead but shaped exactly like the hazard
`kernel/src/fs/fat.rs:467-489` `read_async`: `Box::pin(async move { ... unsafe { core::slice::from_raw_parts_mut(buf_ptr as *mut u8, buf_len) } ... })`. Own comment: "we rely on the caller to ensure it doesn't race. In a real async driver, we would need to pin user memory."
- Driven by `TaskState::Polling`/`SyscallFuture::FileRead`/`pending_future` (`tcb.rs:80-81,143-146,217`); polled by the dummy-waker loop the ADR's Context names (`scheduler.rs:777-819`).
- Only safety net today: task-death cleanup (`scheduler.rs:485-494`) — removing the task from the poll set means the dangling write can never execute. Its own note: "if a real async-DMA driver lands... add a descriptor cancel / frame-unpin point HERE."
- Confirmed dead today: no assignment to `pending_future = Some` or `state = TaskState::Polling` anywhere; `task.rs:1010-1023` documents `file_read` bypasses it entirely.
- **Cancellation impact: YES if ever wired live.** The death guard covers task death only, not a live task abandoning the wait via completion-queue cancellation while the future keeps running and later writes into `buf_ptr` — memory the task has moved on from. **Action: add a "do not wire this to a live syscall without a cancel/unpin point" comment; do not fix the dead code itself in this change.**

## Core IPC hazard: direct writes into a `TaskState::Recv`-registered `buf_ptr` from another task's context
Five sites in `kernel/src/task.rs` copy into/read from another task's raw VA while it sits in `Recv`/`Sending`, justified only by "SAS + it's parked, and we hold SCHEDULER so nothing else can run."
- **Delivery-side (writes into target's `Recv.buf_ptr`) — in scope for this migration:**
  - `task.rs:1227-1233` `ipc_send`
  - `task.rs:1291-1297` `ipc_post_nonblock` (reached from `console_drv.rs:174-208`, the shell input path)
  - `task.rs:1478-1484` `ipc_try_send`
- **Sender-side (reads from sender's `Sending.msg_ptr`) — OUT OF SCOPE, `Sending` state untouched by this migration:**
  - `task.rs:1346-1352` `ipc_recv`
  - `task.rs:1421-1427` `ipc_try_recv`

Today's invariant holds only because the `TaskState` variant *is* the park, checked under the same `SCHEDULER` lock as the copy, single-hart, atomic. **All three delivery-side sites must stop writing directly into the target's buffer if the migration makes that park cancellable.**

## Fix shape already exists in the codebase: stash-then-self-write
- `pending_exit_reason` (`tcb.rs:209-214`, set by `scheduler.rs:543-565`) and the hotswap `PendingMsg` struct (`tcb.rs`, owned `Vec<u8>` + `sender_tid` + `enqueued_tick`) both stash owned data on the `Task` struct and let the *resumed* task copy into its own buffer in its own context, rather than a foreign task writing into it directly.
- **Recommendation adopted in solution design:** route Recv delivery through the existing `pending_msgs` mailbox (already used for the Frozen/busy fallback case) instead of a direct buffer write, for all three delivery-side call sites. This collapses three divergent code paths into one and eliminates the three unsafe sites in the same stroke.

## Pinning registry solves a different problem — not consulted here
`kernel/src/memory/pin.rs` guards frames a *device* may still touch (quarantine on death, explicit driver acknowledge, never a timer). None of `ipc_send`/`ipc_recv`/`ipc_post_nonblock`/`ipc_try_send`/`ipc_try_recv` consult it. Separately, `ipc_borrow_write`/`ipc_borrow_read` (`task.rs:1548-1605`) write into a `Lease.ptr` with no state check and no pin at all — weaker than the informal assumption, but no live caller outside `kernel/src/task/ipc_test.rs`. **Out of scope.**

## Sites ruled out (same-caller synchronous execution, no cross-task park dependency)
`syscall.rs:530-548,1300-1304,1354-1363,1396-1402` (self-writes after own resume), `syscall.rs:2506-2510,2953-2955` (SpawnFromElf/FromMem — synchronous, not parked), `fast_ipc.rs:138-161` (direct call, no TaskState), `drivers/irq_wait.rs` + `syscall.rs:3488-3508` (WaitIrq — atomic flag only, no payload write), hypervisor vCPU register accessors.

## Bottom line
Do not migrate `TaskState::Recv` onto a cancellable completion-queue park until delivery stops writing directly into another task's `buf_ptr`. Fix shape: stash sender id + owned message bytes via the existing `pending_msgs` mailbox, let the resumed receiver copy into its own buffer — mirroring `pending_exit_reason`. `Sending`-side reads are unaffected and out of scope.

## Limitations
`kernel/src/**` only — `cells/services/vfs/**` (userspace filesystem cell) not read. Static reasoning, not fuzzed. Recommend a cancel-a-Recv-then-reuse-the-buffer stress test once migrated.
