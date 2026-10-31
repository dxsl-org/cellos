# CP1 — Phase 02 debt: fail-closed fixes with an observable change

Status: APPROVED by the user 2026-11-01 (all six items selected in the multi-select). Law-1 semantic delta: new refusals or faults at the boundaries listed below, with no ABI layout change.

Source: the writable-global review (`scripts/mutable-globals.toml` debt entries), verified by read-only scouts on 2026-11-01.

| # | Global | Change | Observable delta |
|---|---|---|---|
| 4 | `irq_wait::IRQ_MMIO_BASE` | `WaitIrq` accepts a nonzero `mmio_base` only when `resource_registry::lookup_mmio_owner(mmio_base) == Some(caller)`. Zero stays valid. | An ungranted base gets `PermissionDenied`, where it was stored before and then dereferenced by the riscv64/aarch64 ISR. |
| 10 | `syscall::SHM_HANDLES` | Each `ShmAlloc` frame records its owner `(cell, generation)`. `ShmMap` requires the owner. Root retirement unmaps the identity VA if still mapped, flushes, and frees the frame. | A non-owner `ShmMap` gets `PermissionDenied`. Frames are reclaimed when the owner dies. No current caller. |
| 6 | `iommu_x86::DID_COUNTER` | DIDs are bounded by `GCAP.ND` and reused after an acknowledged teardown. | When DIDs are exhausted, the DMA map returns `Rejected` instead of programming a wrapped, zero or colliding DID. |
| 7 | `svm_registry::NEXT_ASID` | ASIDs are bounded by CPUID `8000_000A.EBX`, reserved before any frame is allocated. | When ASIDs are exhausted, `create_vcpu` is refused with the existing error mapping. |
| 3 | x86 `__stack_top` | A guard page below the BSP boot stack, installed by `ist::install_guards`. | A BSP stack overflow faults instead of corrupting `.bss`. |
| 1 | `cpu_features::HAS_X86_VIRT` | Each AP enters the same SVM/VMX root mode as the BSP before it goes online. | An AP that cannot enter root mode stays offline. |

Internal fix bundled with #4/#10 (same file, no semantic delta): `sys_cap_revoke` takes `CELL_AUTHORITIES` and then `SCHEDULER`, the opposite of `finish_root_retirement` and `register_cell_authority`. Reorder it to the established order.

Rollback: revert each commit; every item is independent.

Evidence ceiling: QEMU/TCG. TCG has no VMX, so #1 can only be exercised on the SVM path.
