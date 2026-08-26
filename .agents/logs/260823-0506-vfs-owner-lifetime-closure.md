# 2026-08-23 — VFS SMP owner-lifetime closure

## What happened
Closed and committed CELLOS-VFS-SMP-006. The owner-lifetime repair binds durable VFS state to a scheduler-owned root task lifecycle rather than assuming CellId equals TID.

## Decisions
- Root TID/generation is the Cell lifetime endpoint; worker exits remain task-local.
- Resolve/watch/cancel owner APIs preserve CallerIdentity and existing syscall ABI while adding append-only VFS-only records.
- VFS leases use exact pending revoke and atomic pin/quarantine transfer.
- Recoverable trap faults and quota-saturated root exits defer retirement under kernel attribution; generic kernel panics remain non-recoverable.

## Verification
- API 90; RV32 release compile; fresh hooks; one-hart VFS 2/2; RV64 SMP VFS 7/7.
- Final quality/security closure passed. RV32 runtime remains host-firmware evidence-gated.

## Next steps
- Phase 07 remains blocked on Phase 03, Phase 04 and Tier 2 native-domain qualification.
- Phase 08 remains predesign-complete and blocked on Phase 03 plus full Phase 07 qualification.
