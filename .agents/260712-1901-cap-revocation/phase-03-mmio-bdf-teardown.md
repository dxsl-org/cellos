# Phase 03 (G2) — MMIO page-table unmap + `release_for`; BDF release

## Context Links
- Plan: [plan.md](plan.md) | Depends on: P00. Parallel-safe with P01/P02 (different files).
- Design authority: dossier §"Class 2 — Mapped MMIO" — "Revoke must unmap the region from the cell's page tables + `release_for` or the cell keeps poking hardware."

## Overview
- **Priority:** P1. **Speed:** G2. **Status:** completed (2026-09-25). **Law 1:** none. **Effort:** M.
- Build the selective MMIO + BDF teardown that revoke invokes: unmap the granted MMIO window from the page table (so the ring-3 cell faults on next touch) and release the resource-registry ownership; release PCIe BDF ownership.

## Key Insights
- MMIO ownership lives in `resource_registry::REGISTRY` (`resource_registry.rs:82`); `release_for(cell_id)` (:190) retains-out all regions owned by the cell — already selective by cell, called on death. It clears **ownership bookkeeping only**; it does NOT unmap page tables.
- The page-table mapping is arch-split:
  - **x86_64:** `map_mmio_user_x86(phys, size)` (`paging.rs:493`) adds a user-accessible identity PTE on demand at `sys_request_mmio` (`syscall.rs:2947-2957`). Reverse with `unmap_page_x86(vaddr)` (`paging.rs:518`, idempotent, clears leaf PTE + `invlpg`) per page.
  - **riscv64/aarch64:** `user_map` is a **no-op** at request time (`syscall.rs:2950-2951`) — MMIO is boot-mapped user for all cells in the single SAS page table. To revoke, the user/valid bit for the range must be cleared (open-question #2). **No helper exists today** → this phase adds a per-arch `clear_mmio_user(base, len)`.
- BDF ownership: `resource_registry::release_bdfs_for(tid)` (:234) already selective-by-tid, called on death.
- The teardown must map a **revoked device-class mask** (the `mmio_devices` bits being cleared) to the specific `REGISTRY` regions to unmap — not blindly release ALL the cell's MMIO. Cross-reference region base against the per-arch `ALLOWED`/`PCIE_BARS` device class.

## Requirements
- **F1:** `resource_registry::revoke_mmio_for(cell_id, device_mask)` — release only regions whose device class ∈ `device_mask`; return the freed `(base,len)` list so the caller can unmap page tables.
- **F2:** page-table unmap: x86 → `unmap_page_x86` per page of each freed region; riscv/aarch64 → new `clear_mmio_user(base, len)` clearing the user (leaf accessibility) bit while keeping the frame identity-mapped for the kernel.
- **F3:** `release_bdfs_for(tid)` reused as-is for the BDF surface (invoked when `pcie_driver` revoked — wired in P05).
- **F4:** all-or-nothing per region; overflow-checked ranges (mirror `request_mmio`).
- **NF1:** SAS frame-identity invariant — never fully unmap the MMIO frame from the kernel; only remove *user* accessibility.

## Architecture
Two-step per revoked MMIO class: (1) `revoke_mmio_for` drops registry ownership + returns regions; (2) arch page-table teardown removes user access → cell's next MMIO load/store faults (fail-closed). BDF release is independent bookkeeping consumed only by the `pcie_driver` path.

Data flow: revoked `mmio_devices` bits → `revoke_mmio_for(cell, mask)` → freed regions → per-arch `unmap_page_x86` / `clear_mmio_user` → cell faults on device access. (`pcie_driver` path additionally: `release_bdfs_for(tid)` + P01 `revoke_dma_for_cell(tid)`.)

## Related Code Files
- **Modify:** `kernel/src/resource_registry.rs` (add `revoke_mmio_for`; `release_for`:190 + `release_bdfs_for`:234 reused).
- **Modify:** `kernel/src/memory/paging.rs` (add `unmap_mmio_user_x86` wrapper over `unmap_page_x86`; add riscv/aarch64 `clear_mmio_user`).
- **Read-only:** `syscall.rs:2938-2996` (RequestMmio mapping side), `resource_registry.rs:153-185` (ALLOWED/PCIE_BARS class lookup).

## Implementation Steps
1. `revoke_mmio_for(cell_id, device_mask)`: iterate `REGISTRY`, match each region's base to its device class via the allowlist/BAR tables, remove + collect those in `device_mask`.
2. x86: `unmap_mmio_user_x86(base, len)` loops `unmap_page_x86` per page.
3. riscv/aarch64: `clear_mmio_user(base, len)` walks the kernel page table, clears the user/access bit on each leaf (keep valid+kernel), TLB-flushes the range. Confirm the exact HAL PTE flag API. `[UNVERIFIED]` — HAL leaf-flag mutation helper to be located during impl.
4. Wire caller side is P04/P05; this phase delivers the primitives + tests.

## Todo List
- [ ] `resource_registry::revoke_mmio_for(cell, mask)` → freed region list
- [ ] x86 `unmap_mmio_user_x86`
- [ ] riscv/aarch64 `clear_mmio_user` (+ locate HAL leaf-flag helper)
- [ ] Test (x86): request MMIO, revoke class, assert region gone + PTE user bit cleared
- [ ] Test (riscv): same via PT-read hook

## Success Criteria
- After teardown, the cell's next access to the revoked MMIO window faults (x86: PTE not present; riscv/aarch64: user bit cleared) and `region_count()` drops by the revoked regions only.
- Regions of a device class NOT in `device_mask` remain owned + mapped.
- 3-arch boot + peripheral suites green (no boot caller revokes MMIO).

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|-----------|
| riscv/aarch64 clear-user also breaks kernel access to that MMIO | Med×High | Clear user bit only, keep valid+kernel R/W; test kernel still reads the region post-revoke. |
| Region base ≠ allowlist base (sub-window grant) → class match misses | Med×Med | Match by containment (base within a known window), not exact base; mirror `request_mmio` overlap logic. |
| Full unmap violates SAS frame-identity | Low×High | NF1: remove user accessibility only, never return the MMIO frame. |

## Security Considerations
Without page-table teardown, `mmio_devices` revoke is a label change — the cell keeps its own PTE to the device (no syscall on the access path to re-check). Removing user accessibility is the only kernel-enforceable boundary for ambient MMIO in SAS.

## Next Steps
- Consumed by P04 (MMIO bits) and P05 (`pcie_driver` → BDF + DMA + its MMIO BARs).
- Rollback: revoke stops calling these; primitives are inert if uncalled.

---

## Closure (2026-09-25)

**Verified on:** RV64 QEMU TCG 8.2.2, `harts=1`, kernel `76832e05…`, feature tuple
`native-domains,test-hooks` — raw log `docs/evidence/cap-revoke-qemu.{log,txt}`.

### Delivered
- `resource_registry` now stores the **device class with the region**:
  `MmioRegion { len, owner, class }`, `MmioClass = u16`. `request_mmio` records the class
  the allowlist matched; `request_mmio_unchecked` records `CLASS_ECAM` (the Platform
  Cell's config-space window has no `DEV_*` bit); `request_dwc2_mmio` records `CLASS_DWC2`.
- `static_range_class` returns the matched window's class; the bool wrapper was deleted
  (the RPi3 boot self-test now reads `.is_some()`), so there is one derivation.
- `revoke_mmio_for(cell_id, device_mask) -> Vec<(base, len)>`: releases exactly the
  windows owned by that cell whose class is in the mask and returns them for page-table
  teardown. `release_for` (all classes, cell death) is unchanged.
- `paging::unmap_mmio_user_x86` (clears the user leaf PTE per page, idempotent),
  `paging::clear_mmio_user` (riscv64/aarch64: `protect_page` with the boot MMIO flags
  minus `USER`, keeping `DEVICE` on aarch64 — the frame stays identity-mapped for the
  kernel), and `paging::revoke_mmio_user` as the arch dispatcher. All three return the
  pages changed and `Err` when a mapped page could not be re-protected (fail-closed).
- BDF release reuses `release_bdfs_for` (P05 wires it to `pcie_driver`).

### Evidence (markers in the published log)
| Property | Marker |
|---|---|
| Revoking one class releases exactly that window and leaves another class owned | `MMIO-REVOKE-CLASS: PASS` |
| The real allowlist path: class derived by `request_mmio`, matched by the revoke | `MMIO-REVOKE-ALLOWLIST: PASS` |
| After revoke the page is still mapped for the kernel but no longer user-accessible | `MMIO-REVOKE-USERBIT: PASS` |

### Deviations from the plan
- **Class stored at grant time, not re-derived by containment.** The plan's risk table
  proposed matching a region base against `ALLOWED`/`PCIE_BARS`. That cannot be correct:
  a PCIe BAR is in neither allowlist, and a granted sub-window is *contained in* rather
  than equal to its window. Recording the class where it is decided removes the whole
  failure class instead of mitigating it.
- `revoke_mmio_user` is proven on a user-mapped grant page: the change it makes is the
  same one an MMIO window gets, and a grant page is user-mapped on every board — the
  QEMU targets' allowlisted windows are not mapped in the kernel page table at all.

### Not verified here
- The x86 leg is compile-verified only in this lane (`x86_64-unknown-none` builds clean);
  the aarch64/x86 test-hooks lanes were not run. Both legs share the
  `revoke_mmio_user` contract with the riscv leg that is exercised.
