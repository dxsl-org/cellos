# Peer-Death CQ Readiness Audit

## Verdict

Not ready for implementation. The completion queue exists and is correct for its current `NET_RX` owner, but there is still no production async IPC submit path that reserves a CQ slot and registers a dependency on a target task death. Implementing peer-death completion now would either invent the wrong owner or freeze result semantics before the real caller exists.

## Current Proven State

- `WaitCompletion = 242` is already shipped and defined as "submission from the caller's own context"; it returns `1` when a completion record was written and `0` on timeout/empty wait. Evidence: `libs/api/src/abi/syscall.rs:409`, `libs/api/src/abi/syscall.rs:416`, `libs/api/src/abi/syscall.rs:429`.
- The completion queue is kernel-owned heap state, not a grant, specifically so in-flight completions always have a landing place. Evidence: `kernel/src/task/completion.rs:1`, `kernel/src/task/completion.rs:3`, `kernel/src/task/completion.rs:7`, `kernel/src/task/completion.rs:102`.
- A slot identifier is cell-local by design because task IDs are the wrong identity for completions. Evidence: `kernel/src/task/completion.rs:47`, `kernel/src/task/completion.rs:49`, `kernel/src/task/completion.rs:51`; `libs/api/src/abi/completion.rs:4`, `libs/api/src/abi/completion.rs:7`.
- The only production reservation path today is `wait_completion()`, which reserves a slot, registers the waiter, and arms `NET_RX`. Evidence: `kernel/src/task/completion_wait.rs:67`, `kernel/src/task/completion_wait.rs:85`, `kernel/src/task/completion_wait.rs:86`, `kernel/src/task/completion_wait.rs:87`.
- `WaitCompletion` currently accepts exactly one source bit, and the only shipped source bit is `NET_RX`. Evidence: `kernel/src/task/completion_wait.rs:73`, `kernel/src/task/completion_wait.rs:75`; `libs/api/src/abi/syscall.rs:796`, `libs/api/src/abi/syscall.rs:802`.
- The current owner for that source is the serialized `NET_RX` reservation object. It displaces the old reservation by completing it as `RESULT_ABANDONED`, and the interrupt path only completes an already-armed slot. Evidence: `kernel/src/task/waker.rs:6`, `kernel/src/task/waker.rs:18`, `kernel/src/task/waker.rs:56`, `kernel/src/task/waker.rs:89`; `kernel/src/task/waker/net_rx_reservation.rs:35`, `kernel/src/task/waker/net_rx_reservation.rs:57`, `kernel/src/task/waker/net_rx_reservation.rs:73`, `kernel/src/task/waker/net_rx_reservation.rs:114`.
- Timeout/unwind cleanup exists only for that `NET_RX` wait shape: waiter registration is RAII-cleared, a timed-out wait disarms the source, releases its reservation, and drains a raced completion if needed. Evidence: `kernel/src/task/completion_wait.rs:26`, `kernel/src/task/completion_wait.rs:40`, `kernel/src/task/completion_wait.rs:132`, `kernel/src/task/completion_wait.rs:137`, `kernel/src/task/completion_wait.rs:146`.
- Completion wakes are deferred and queue-local; they do not run from interrupt context. Evidence: `kernel/src/task/completion.rs:21`, `kernel/src/task/completion.rs:24`, `kernel/src/task/completion.rs:359`, `kernel/src/task/completion.rs:364`.
- Even the boot self-test says nothing real is migrated yet; it is proving queue invariants ahead of the first production caller. Evidence: `kernel/src/task/completion_selftest.rs:3`, `kernel/src/task/completion_selftest.rs:4`, `kernel/src/main.rs:602`.

## Death-Path Trace

- `exit_task()` currently removes the dying task, clears service/input registrations, wakes tasks stuck in `TaskState::Sending { target = tid }`, and returns `usize::MAX` to those senders. Evidence: `kernel/src/task/scheduler.rs:495`, `kernel/src/task/scheduler.rs:503`, `kernel/src/task/scheduler.rs:507`, `kernel/src/task/scheduler.rs:512`, `kernel/src/task/scheduler.rs:521`.
- `exit_task()` also wakes `Wait(tid)` waiters and stores the exit reason into `reply_value`. Evidence: `kernel/src/task/scheduler.rs:533`, `kernel/src/task/scheduler.rs:536`, `kernel/src/task/scheduler.rs:539`.
- `NotifyOnExit` is a separate one-shot watcher registry. Death delivery wakes parked `Recv` watchers or appends `(dead_tid, exit_reason)` into `pending_deaths` for later `Recv`. Evidence: `kernel/src/task/scheduler.rs:544`, `kernel/src/task/scheduler.rs:548`, `kernel/src/task/scheduler.rs:552`, `kernel/src/task/scheduler.rs:563`; `kernel/src/task/syscall.rs:2201`, `kernel/src/task/syscall.rs:2239`; `kernel/src/task/syscall.rs:1345`, `kernel/src/task/syscall.rs:1352`.
- None of the death paths touch `CompletionQueue`, because no production completion owner currently depends on a peer task. The queue wake machinery only knows about its registered waiter TID, not any target dependency. Evidence: `kernel/src/task/completion.rs:283`, `kernel/src/task/completion.rs:292`, `kernel/src/task/completion.rs:306`; `kernel/src/task/scheduler.rs:512`, `kernel/src/task/scheduler.rs:544`.

## Missing Before Peer-Death CQ Can Exist

### 1. Submission Owner

The first real owner must be the future async IPC submit path, not `NET_RX`.

- The queue contract requires reservation at submit time, from the submitter's own context, before anything is in flight. Evidence: `kernel/src/task/completion.rs:13`, `kernel/src/task/completion.rs:131`, `kernel/src/task/completion.rs:134`.
- `NET_RX` satisfies that today only because `WaitCompletion` itself acts as submitter for a hardware level event. Peer death is not a hardware source; it is a dependency of an already-submitted async IPC operation. Evidence: `kernel/src/task/completion_wait.rs:3`, `kernel/src/task/completion_wait.rs:7`.

Required future owner contract:

- reserve slot in the async IPC submit syscall path;
- register `(queue handle, slot, waiter tid, target identity)` in a kernel-internal dependency table;
- unregister and release on any submit failure before the operation becomes observable;
- hand the same reservation to the eventual reply/peer-death completion path.

### 2. Target-Dependency Registry

There is no internal table today that says "slot X on queue Q depends on target task Y dying."

Required minimum shape:

- key by `target_tid`;
- store `target_generation` alongside `target_tid`;
- store `Arc<CompletionQueue>` and `SlotId`;
- optionally store `waiter_tid` only if an explicit death-path wake is ever needed outside normal CQ delivery.

Why generation is required:

- The queue contract already rejects task IDs as completion identity because they are reused and cannot distinguish operations. Evidence: `kernel/src/task/completion.rs:49`, `kernel/src/task/completion.rs:50`.
- The task model already has `cell_generation`, minted per cell specifically so future ID reuse cannot rebind old state to a new cell. Evidence: `kernel/src/task/tcb.rs:331`, `kernel/src/task/tcb.rs:338`, `kernel/src/task/tcb.rs:345`, `kernel/src/task/tcb.rs:383`, `kernel/src/task/tcb.rs:438`.
- `deliver_pending_wakes()` intentionally treats unreachable queues as the safe outcome because waking a reissued TID would be wrong. Evidence: `kernel/src/task/completion.rs:372`, `kernel/src/task/completion.rs:375`.

Conclusion: a peer-death dependency table that stores only raw `target_tid` would be knowingly weaker than the rest of the identity model.

### 3. Cleanup Points

The future owner must define all unregister sites up front.

- Submit refusal path: if queue reservation or dependency-table insertion fails, return an ordinary error and release the slot immediately. Evidence for the reserve/refuse model: `kernel/src/task/completion.rs:133`, `kernel/src/task/completion.rs:137`.
- Submit cancellation path: if the async IPC operation times out, is aborted, or is displaced by another owner decision, unregister the dependency before freeing/reusing the slot. The current `NET_RX` wait proves why timeout cleanup must release the reservation. Evidence: `kernel/src/task/completion_wait.rs:16`, `kernel/src/task/completion_wait.rs:17`, `kernel/src/task/completion_wait.rs:137`.
- Target death path: `exit_task()` must complete every registered slot for that dead target before any future ID reuse could make a stale dependency point at another cell. Current `exit_task()` has no such loop. Evidence: `kernel/src/task/scheduler.rs:512`, `kernel/src/task/scheduler.rs:544`.
- Waiter death path: if the waiting cell dies after submission, the dependency entry must be removed or rendered unreachable. Keeping only the queue `Arc` is safe but bounded-leaky, matching the queue's existing safety story. Evidence: `kernel/src/task/completion.rs:97`, `kernel/src/task/completion.rs:99`, `kernel/src/task/completion.rs:100`.

## Result Semantics

Recommendation:

- treat peer death as a negative completion result, not as a recycled exit code;
- keep actual child/supervised exit reasons on `Wait(tid)` / `NotifyOnExit`, which already encode that contract;
- do not overload `ViCompletion.slot` or the reserved word to carry target identity.

Why:

- `Completion.result` is already explicitly reserved for source-defined semantics, with negative values reserved for errors. Evidence: `kernel/src/task/completion.rs:67`, `kernel/src/task/completion.rs:69`; `libs/api/src/abi/completion.rs:19`, `libs/api/src/abi/completion.rs:20`.
- `Wait(tid)` and `NotifyOnExit` already return exit reasons, so CQ does not need to become a second exit-status ABI just to report "peer disappeared." Evidence: `kernel/src/task/scheduler.rs:539`, `kernel/src/task/scheduler.rs:559`; `kernel/src/task/syscall.rs:1353`.

Recommended initial semantic:

- peer died before the async IPC operation completed => one negative completion result such as `RESULT_PEER_GONE`;
- submitter/local timeout => syscall returns `0` and releases the reservation, same as current `WaitCompletion`;
- displaced reservation => keep existing `RESULT_ABANDONED`.

## Law-1 / ABI Stop Gates

These changes require explicit Law-1 handling because they touch `libs/api/src/abi/*` or frozen ABI semantics.

1. Adding any new completion source bit such as `PEER_DEATH` under `api::syscall::events` is an ABI change. Evidence: `libs/api/src/abi/syscall.rs:794`, `libs/api/src/abi/syscall.rs:800`.
2. Changing `ViCompletion` layout, length, reserved word meaning, or parse/write contract is an ABI change. Evidence: `libs/api/src/abi/completion.rs:23`, `libs/api/src/abi/completion.rs:44`, `libs/api/src/abi/completion.rs:60`.
3. Publishing a new named result constant in `libs/api/src/abi/completion.rs` for callers to distinguish peer death is an ABI/API contract change, even if the struct layout stays fixed. Evidence: `libs/api/src/abi/completion.rs:36`, `libs/api/src/abi/completion.rs:42`.
4. Adding a new syscall, widening `WaitCompletion` arguments, or exposing explicit register/unregister operations in `libs/api/src/abi/syscall.rs` is an ABI change. Evidence: `libs/api/src/abi/syscall.rs:409`, `libs/api/src/abi/syscall.rs:411`.

These changes do not require Law 1 by themselves:

- a kernel-internal dependency registry;
- scheduler/internal `exit_task()` completion over that registry;
- generation tagging kept entirely internal to the kernel.

Law-1 basis:

- project rule: changes under `crate::abi` require the special confirmation path. Evidence: `docs/code-standards.md:11`; `libs/api/src/services.rs:5`.

## Decision

Do not implement peer-death CQ in the current slice.

Prepare the future async IPC migration to own:

- reservation;
- target `(tid, generation)` registration;
- timeout/unregister cleanup;
- peer-death completion result policy.

Until that owner exists, any Phase 07 implementation would be architectural guesswork.
