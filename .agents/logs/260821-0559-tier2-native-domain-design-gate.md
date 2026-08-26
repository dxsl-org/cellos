# 2026-08-21 — Tier 2 native-domain design gate

## What happened
Phase 4 added Spec 22 as the mandatory design and negative-test gate for any
future Tier 2 native-domain implementation, then synchronized living docs.

## Decisions
- Keep Tier 2 accepted but unimplemented; unsigned admission is never containment.
- Preserve the Tier 1 SAS-to-SAS path without mandatory MMU-root switches.
- Require private-root lifetime, recoverable syscall copies, synchronous grant
  revoke, DMA fencing, hostile negative tests, and atomic rollback controls.
- Defer manifest v3 and all runtime implementation to a separately approved plan.

## Lessons
- Page-table isolation is incomplete unless syscall pointer faults recover safely.
- Domain teardown must quiesce every hart on a safe root before freeing tables.
- Build capability and runtime admission rollback need separate state contracts.

## Next steps
- Prepare a separate implementation plan covering Spec 02/17 addenda, MMU
  backends, scheduler/TCB changes, copied IPC/grants, DMA, admission, and tests.
