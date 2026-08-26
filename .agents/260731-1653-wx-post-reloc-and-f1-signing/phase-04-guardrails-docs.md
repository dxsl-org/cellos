# Phase 04 — Documentation Guardrails for Deferred Items

## Context Links

- Plan: [plan.md](plan.md) · Depends on [Phase 02](phase-02-route-delivery-pending-msgs.md) (can run parallel to Phase 03)
- `docs/specs/03b-async-reactor-adr.md` — Consequences section
- `kernel/src/fs/fat.rs:467-489` — dead-code `read_async` hazard
- `kernel/src/task.rs:1284-1289` — `ipc_post_nonblock` ignored-mask latent bug
- `kernel/src/task/scheduler.rs` — `exit_task`'s Recv-wake logic (reply-waiter hole)
- `kernel/src/task.rs:1548-1605` — `ipc_borrow_write`/`ipc_borrow_read`, no live caller
- `kernel/src/task/syscall.rs` — `RecvScatter`, if Phase 01 determined it has a pre-existing,
  unrelated issue rather than something this plan should fix

## Overview

- **Priority:** P2 · **Status:** completed · **Risk:** low
- Record what this change deliberately does not fix, and record why the completion-queue
  migration itself was deferred, so neither is silently forgotten.

## Requirements

**Functional (all doc/comment-only, no behavior change)**
1. `fat.rs:467-489` `read_async` — comment: must not be wired to a live syscall without a
   cancel/unpin point.
2. `task.rs:1284-1289` `ipc_post_nonblock` — comment flagging the ignored-`mask` behavior as a
   known latent issue, tracked separately.
3. The reply-waiter-not-covered-by-`exit_task` hole — comment at the relevant `exit_task` logic
   noting a plain reply-waiter (not a `NotifyOnExit` subscriber) is not woken if its target dies;
   pre-existing, orthogonal to this change.
4. `ipc_borrow_write`/`ipc_borrow_read` — comment noting no live caller exists outside tests.
5. `RecvScatter` — comment the verified pre-existing defect: after `ipc_recv` returns `Ok(0)` it
   does not yield, immediately scatters/returns, and leaves `TaskState::Recv` holding a temporary
   kernel-buffer pointer. The producer-routing fix removes the foreign write hazard, but functional
   repair requires a separately scoped blocking/lifecycle change.
6. `docs/specs/03b-async-reactor-adr.md` — append a short Consequences note: the buffer-pinning
   audit item this ADR required is addressed (three delivery sites no longer write into a foreign
   task's buffer); migrating `TaskState::Recv`'s wait mechanism onto the completion queue itself
   was attempted, found to require giving IPC recv its own slot space independent of NET_RX's
   shared queue (plus per-teardown-path bookkeeping the variant-embedded design couldn't support),
   and is deferred as a separately-scoped future effort.

## Related Code Files

**Modify**
- `kernel/src/fs/fat.rs`
- `kernel/src/task.rs`
- `kernel/src/task/scheduler.rs`
- `kernel/src/task/syscall.rs` (only if item 5 applies)
- `docs/specs/03b-async-reactor-adr.md`

## Implementation Steps

1. Add the code comments listed in Requirements 1-5 (as applicable), each citing this
   plan/date (2026-07-31) for a future reader to find context without an inline essay, per this
   repo's Rust comment standards.
2. Append the ADR addendum (Requirement 6).

## Todo List

- [x] Comment on `fat.rs` `read_async`
- [x] Comment on `ipc_post_nonblock` ignored-mask
- [x] Comment on reply-waiter `exit_task` hole
- [x] Comment on `ipc_borrow_write`/`ipc_borrow_read`
- [x] Comment on the verified `RecvScatter` missing-yield/temp-buffer lifecycle defect
- [x] ADR Consequences addendum

## Evidence

- Guardrails are present at each deferred production-code site and the ADR Consequences section
  records both the completed buffer-pinning fix and the deferred completion-queue migration.

## Success Criteria

- Every deferred item has a durable, discoverable marker at the exact site a future reader would
  need it, not buried only in this plan's `.agents/` directory.

## Risk Assessment

- None — this phase changes no executable logic.

## Security Considerations

- None.

## Next Steps

- None beyond this plan. If the completion-queue migration is revisited later, it should start
  from [research/red-team-findings.md](research/red-team-findings.md) rather than repeating the
  same design.

## Assumptions

- None — this phase only records facts already verified during research and review.
