# ADR Stub: Peer-Death Completion Ownership

## Status

Proposed stub for a future implementation phase. Not approved for code yet.

## Context

- `WaitCompletion` and the per-cell `CompletionQueue` already exist, but the only production source is `NET_RX`. Evidence: `kernel/src/task/completion_wait.rs:75`; `libs/api/src/abi/syscall.rs:802`.
- The queue contract requires the slot to be reserved from the submitter's own context before the asynchronous source can report a result. Evidence: `kernel/src/task/completion.rs:13`, `kernel/src/task/completion.rs:14`.
- `exit_task()` currently handles `Sending`, `Wait(tid)`, and `NotifyOnExit`, but no completion dependency registry. Evidence: `kernel/src/task/scheduler.rs:512`, `kernel/src/task/scheduler.rs:533`, `kernel/src/task/scheduler.rs:544`.

## Decision

Future peer-death completions will be owned by the first real async IPC submit path, not by `WaitCompletion` itself and not by `NotifyOnExit`.

That owner must:

1. Reserve a CQ slot at submit time.
2. Register a kernel-internal dependency keyed by `(target_tid, target_generation)`.
3. Unregister and release on submit failure, timeout, or local cancellation.
4. Complete the reserved slot exactly once on either normal reply or peer death.
5. Keep all target identity tracking kernel-internal unless a later phase deliberately chooses a Law-1 ABI change.

## Consequences

- No scheduler or CQ code should be changed until the async IPC owner exists.
- The first implementation phase must include an `exit_task()` sweep over the new dependency table.
- The implementation should prefer a negative completion result like `RESULT_PEER_GONE` over overloading child exit reasons into CQ.
- If the future phase needs a new completion source bit, a new named result constant, or any `ViCompletion` layout change, it must stop for Law-1 confirmation first.

## Options Considered

### Option A: Implement peer-death completion now in scheduler/CQ

Rejected.

- It would invent an owner before the real async IPC submit path exists.
- It would force cleanup semantics without knowing the final submit/cancel state machine.

### Option B: Reuse `NotifyOnExit`

Rejected.

- `NotifyOnExit` is watcher-oriented supervision, not per-submission completion ownership.
- It carries child death information, not the lifecycle of an async IPC operation.

### Option C: Wait for async IPC migration, then attach peer-death to that owner

Accepted.

- Matches the queue contract.
- Keeps result semantics tied to the first real caller.
- Allows generation-safe dependency registration from the start.

## Open Decision

Whether the first caller needs a publicly named ABI result such as `RESULT_PEER_GONE`, or whether the initial migration can keep peer-death as an internal negative result until multiple callers need to distinguish it.
