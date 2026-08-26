# Phase 01 (G2) — Implement `iommu::unmap_dma` + selective DMA-domain teardown

## Context Links
- Plan: [plan.md](plan.md) | Depends on: P00 (honest baseline), P-TRUST (DMA-anywhere invariant)
- Design authority: dossier §"Class 2 … IOMMU/DMA" — "`iommu.rs:51 unmap_dma` is a no-op stub … implementing it (+ IOTLB flush) is the FIRST thing; everything else assumes DMA can actually be torn down."

## Overview
- **Priority:** P1 (foundational). **Speed:** G2. **Status:** pending. **Law 1:** none. **Effort:** S.
- Turn the `unmap_dma` no-op stub into a real per-range teardown (zero the IOMMU PT/SLPT entry + IOTLB-flush the page) on both arches, and expose a **selective** whole-domain teardown callable from revoke (not only from cell death).

## Key Insights
- `iommu::unmap_dma(_iova, _size)` is a **no-op** (`iommu.rs:51-53`). Comment claims "handled by cleanup_cell on exit" — true for death, false for runtime revoke.
- `cleanup_cell(tid)` (`iommu.rs:58-63`) already does the FULL per-cell teardown: riscv `unmap_cell` (`iommu_riscv.rs:342`: PSCID TLB invalidate + IOFENCE + zero DDT per BDF), x86 `unmap_cell_domain` (`iommu_x86.rs:360`: DSI IOTLB flush + zero context entries). **This is the correct granularity for revoking `pcie_driver`** (the cell loses its whole DMA domain).
- Per-range primitives partially exist: x86 `unmap_range_for_cell` (`iommu_x86.rs:393`) only page-flushes IOTLB — it does NOT zero the SLPT entry, so DMA to that IOVA still resolves. riscv has no per-range unmap. Both are incomplete → `unmap_dma` cannot just forward to them yet.
- Both arch domain maps key on `tid` (`VTD_DOMAINS`, `RISCV_DOMAINS`), so a per-cell selective call is natural.

## Requirements
- **F1:** `unmap_dma(iova, size)` zeros the leaf IOMMU PT/SLPT entry for `[iova, iova+size)` AND page-selective-flushes IOTLB, on riscv64 + x86_64.
- **F2:** x86 `unmap_range_for_cell` extended to zero the SLPT leaf (currently flush-only) so F1 holds.
- **F3:** riscv `Sv39IommuPt` gains an `unmap_range` mirroring its `map_range`; wire a `unmap_range_for_cell`.
- **F4:** expose `iommu::revoke_dma_for_cell(tid)` = the `cleanup_cell(tid)` teardown path, callable from revoke (rename/re-export `cleanup_cell` or add a thin alias — same code, no fork).
- **NF1:** IOFENCE / IOTLB completion MUST finish before any freed frame is reused (frame-quarantine invariant already honored by `unmap_cell`; preserve it).

## Architecture
Two layers:
1. **Per-range** (`unmap_dma`) — for future partial DMA-grant revoke and to make the stub honest. Zero PT entry then flush that page.
2. **Per-domain** (`revoke_dma_for_cell` = `cleanup_cell` semantics) — what P04/P05 call when `pcie_driver` is revoked: tears down the entire domain + BDFs. Reusing `cleanup_cell` is the dossier's "callable selectively" directive.

Data flow (revoke path, wired in P04/P05): revoked `pcie_driver` bit → `revoke_dma_for_cell(tid)` → arch `unmap_cell`/`unmap_cell_domain` → IOTLB flush + zero DDT/context → device DMA faults on next access.

## Related Code Files
- **Modify:** `kernel/src/task/drivers/iommu.rs` (`unmap_dma` :51; add `revoke_dma_for_cell` alias to `cleanup_cell`).
- **Modify:** `kernel/src/task/drivers/iommu_x86.rs` (`unmap_range_for_cell` :393 — zero SLPT leaf, not just flush).
- **Modify:** `kernel/src/task/drivers/iommu_riscv.rs` — add `unmap_range_for_cell` + `Sv39IommuPt::unmap_range`.
- **Read-only:** `iommu_x86.rs:122-165` (IOTLB flush helpers), `iommu_riscv.rs:342-360` (unmap_cell reference).

## Implementation Steps
1. x86: extend `unmap_range_for_cell` to walk the domain SLPT and zero the leaf entry for each page in range, then `iotlb_flush_page`.
2. riscv: add `Sv39IommuPt::unmap_range` (mirror `map_range`); add `unmap_range_for_cell` that zeroes entries + `invalidate_pscid_tlb` + `issue_iofence`.
3. `iommu::unmap_dma` → dispatch to the arch `unmap_range_for_cell` with the cell tid resolved by caller (extend signature to `unmap_dma(tid, iova, size)` OR keep kernel-domain 0 wrapper — pick tid-aware to match `map_dma_for_cell`).
4. Add `iommu::revoke_dma_for_cell(tid)` = call `cleanup_cell(tid)` (or re-export). Document it is the runtime-revoke entry, identical teardown to cell death.
5. Unit/integration: map a range for a fake tid, `unmap_dma`, assert the entry is zeroed (test hook reading PT) — or assert via IOMMU-fault on a follow-up DMA in the x86 nvme suite.

## Todo List
- [ ] x86 `unmap_range_for_cell` zeroes SLPT leaf + flushes
- [ ] riscv `Sv39IommuPt::unmap_range` + `unmap_range_for_cell`
- [ ] `iommu::unmap_dma` forwards to arch per-range unmap (tid-aware)
- [ ] `iommu::revoke_dma_for_cell(tid)` selective whole-domain alias
- [ ] Test: unmapped IOVA no longer translates (fault or PT-read hook)

## Success Criteria
- After `unmap_dma`, a DMA to the unmapped IOVA faults (IOMMU) instead of resolving — verified on x86 (VT-d) via the nvme suite under isolation, and on riscv via PT-read test hook.
- `revoke_dma_for_cell(tid)` produces byte-identical DDT/context state to a `cleanup_cell(tid)` on death.
- 3-arch boot + x86 nvme 3/3 stay green (drivers that never revoke are unaffected).

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|-----------|
| Freeing/zeroing entry before IOTLB flush completes → device DMAs into reused frame | Low×**High** | Preserve `unmap_cell`'s ordering: flush + IOFENCE BEFORE any frame release; per-range path flushes before returning. |
| x86 SLPT walk zeroes a shared upper table | Low×High | Zero leaf only; never free intermediate SLPT pages during range unmap. |
| riscv `unmap_range` diverges from `map_range` page-size assumptions | Med×Med | Mirror `map_range` exactly (same level/step); add symmetry test. |

## Security Considerations
This is the single most dangerous surface (dossier): until `unmap_dma` is real, revoking `PcieDriverCap` leaves DMA-anywhere live — the runtime half of the invariant P-TRUST closes at spawn. Frame-quarantine ordering is a hard safety invariant, not an optimization.

## Next Steps
- Enables P04/P05 to wire DMA teardown into revoke.
- Rollback: revert `unmap_dma` to the stub — cell-death `cleanup_cell` still works; only runtime revoke loses the primitive.
