# Phase 05 (G2) — Add `cap_mask` ABI bits for privileged caps + wire their teardown (LAW 1)

## Context Links
- Plan: [plan.md](plan.md) | Depends on: **P01, P03, P04** + **P-TRUST** (`.agents/260712-1100`, adds `pcie_driver`/`platform`/`supervisor`/`cell_store_region` to `CapSet`).
- Design authority: dossier §"Two-speed recommendation" #2 (widen back to those bits) + §"Reachability today".

## Overview
- **Priority:** P2 (last; needs ABI change + P-TRUST landed). **Speed:** G2. **Status:** completed (2026-09-25). **Effort:** S.
- **🔴 LAW 1 — requires 2x user confirmation.** Adds new capability-mask bits to `libs/api/src/abi/syscall.rs` (`cap_mask`), the frozen kernel↔cell ABI. Do NOT proceed without explicit double confirmation.
- Make `pcie_driver`, `platform`, `supervisor` revocable end-to-end: define their mask bits, then dispatch their eager teardown (DMA domain + BDF + their MMIO BARs).

## Key Insights
- `cap_mask` today (`libs/api/src/abi/syscall.rs:357-376`): BLOCK_IO(bit0), NETWORK(1), SPAWN(2), HYPERVISOR(3), MMIO_SHIFT(8), BLKREGION_SHIFT(16). **Bits 4-7 are free** — assign PCIE_DRIVER(4), PLATFORM(5), SUPERVISOR(6) (bit7 reserved).
- P-TRUST folds these into `CapSet` (`cap.rs:118`) + TCB already has `pcie_driver_cap`/`platform_cap`/`supervisor_cap` (`tcb.rs:221-224`). So the TCB-field clear is a one-liner per bit; the hard part (teardown) is P01/P03.
- `pcie_driver` revoke teardown = `iommu::revoke_dma_for_cell(tid)` (P01) + `release_bdfs_for(tid)` (P03/registry) + `revoke_mmio_for` for its claimed BARs (P03). `platform`/`supervisor` are authority gates (syscall-mediated, Class-1-like) — clearing the TCB field + lazy re-check suffices, BUT `platform` may own the ECAM MMIO window (via `request_mmio_unchecked`) → also needs `revoke_mmio_for`.
- `supervisor_cap` is "set ONLY by kernel init, never propagated" (`tcb.rs:216-218`); revoking it is purely a TCB clear (no ambient resource) — Class-1.

## Requirements
- **F1 (Law 1):** add `PCIE_DRIVER = 1<<4`, `PLATFORM = 1<<5`, `SUPERVISOR = 1<<6` to `cap_mask`. Update any `cap_mask` doc/`ALL`-style constant. `git diff libs/api` will be non-empty — this is the sanctioned exception.
- **F2:** revoke dispatch: `PCIE_DRIVER` → `revoke_dma_for_cell` + `release_bdfs_for` + `revoke_mmio_for(DEV_PCIE)`; `PLATFORM` → `revoke_mmio_for` (ECAM window) + clear field; `SUPERVISOR` → clear field (Class-1).
- **F3:** remove these three from P00's reject-filter now that teardown exists.
- **F4:** notify via `AppEvent::CapRevoked{mask}` (P04) including the new bits.
- **F5:** Gate — only `SpawnCap` holders may revoke (existing Gate-1); consider whether revoking `SUPERVISOR` needs a higher gate (a supervisor revoking another supervisor). Flag: default keep Gate-1, document.

## Architecture
Extends the P04 dispatch table with three bits. `PCIE_DRIVER` is the heavy Class-2 case (DMA + BDF + BARs = the P-TRUST DMA-anywhere surface, now revocable at runtime). `PLATFORM`/`SUPERVISOR` are light (field clear + one MMIO release for platform).

Data flow: caller mask (now incl. bits 4-6) → gate → per-bit teardown (pcie: DMA/BDF/BAR; platform: ECAM MMIO; supervisor: none) → clear TCB field → audit → notify.

## Related Code Files
- **Modify (LAW 1):** `libs/api/src/abi/syscall.rs` (`cap_mask` :357-376 — 3 new bits).
- **Modify:** `kernel/src/task/syscall.rs` (CapRevoke arm — dispatch the 3 bits; clear `pcie_driver_cap`/`platform_cap`/`supervisor_cap`).
- **Read-only:** `cap.rs` CapSet (post-P-TRUST), `tcb.rs:221-224`, P01 `revoke_dma_for_cell`, P03 `revoke_mmio_for`/`release_bdfs_for`.

## Implementation Steps
1. **Obtain 2x user confirmation for the `libs/api` change.** Do not start until granted.
2. Add the 3 mask bits + doc comment cross-referencing the TCB/CapSet fields.
3. CapRevoke arm: for each of the 3 bits, clear its TCB field and (pcie/platform) run its teardown.
4. Remove the 3 from P00's reject-filter.
5. Extend the revoke notify mask to carry the new bits.
6. Decide `SUPERVISOR` gate (default Gate-1 + doc); add negative test if a higher gate is chosen.

## Todo List
- [ ] 🔴 2x confirmation for `libs/api` cap_mask change
- [ ] `PCIE_DRIVER`/`PLATFORM`/`SUPERVISOR` mask bits
- [ ] pcie teardown dispatch (DMA + BDF + BAR MMIO)
- [ ] platform (ECAM MMIO release) + supervisor (field clear) dispatch
- [ ] Remove the 3 from P00 reject-filter
- [ ] Test: revoke PCIE_DRIVER from a driver cell → DMA faults, BDF released, BAR unmapped, `CapRevoked` received
- [ ] Test: `git diff libs/api` shows only the sanctioned 3-bit addition

## Success Criteria
- Revoking `PCIE_DRIVER` at runtime produces the same DMA-teardown end-state as cell death (P01 oracle) + BDF/BAR released, and a follow-up device DMA faults.
- Revoking `PLATFORM`/`SUPERVISOR` clears the field; the next gated syscall (`sys_register_pcie_bar` / `sys_freeze_cell`) from the target fails closed.
- 3-arch boot + x86 nvme suite green (driver cells that are never revoked keep their caps; ABI addition is backward-compatible — new bits default off).

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|-----------|
| **Law 1 ABI break** — a cell/kernel mismatch on cap_mask | Low×High | Additive bits only (unused = 0); bump no existing bit; 2x confirmation; verify all `cap_mask` consumers (kernel revoke arm is the only reader). |
| Revoking `pcie_driver` from a live driver mid-DMA | Med×High | Reuse P01 frame-quarantine ordering (flush+IOFENCE before any free); Gate-2-style guard could extend to active driver cells if needed. |
| Revoking `supervisor` from init bricks lifecycle mgmt | Low×High | `supervisor_cap` is init-only; consider disallowing self/init revoke (mirror `target_tid == caller_id` guard at `syscall.rs:1423`). |
| P-TRUST not landed → CapSet lacks the bits | — | Hard dependency: P05 MUST follow P-TRUST; plan sequences it last. |

## Security Considerations
This closes the runtime half of the DMA-anywhere invariant: P-TRUST prevents a cell from *acquiring* `PcieDriverCap` outside its ceiling at spawn; P05 lets a supervisor *remove* it at runtime with real teardown. Together they make `PcieDriverCap` a fully governed, revocable authority rather than a one-way grant. The `libs/api` change is the minimum necessary ABI surface and is strictly additive.

## Next Steps
- Completes runtime revocation; `sys_cap_revoke` now honestly revokes every capability it accepts.
- Rollback: remove the 3-bit dispatch + reinstate P00 rejects; revert the `libs/api` bits (additive, safe to drop if unused).
- Follow-up (out of scope): dossier's "service RegisterService stale-tid" is Class-1 partial (`clear_tid` on death) — a lazy re-lookup on the client side, not part of this plan.

---

## Closure (2026-09-25)

**Law-1 confirmation:** granted twice, as this phase requires — the user's instruction to
implement P01-P05, and an explicit second confirmation on 2026-09-25 of the exact three-bit
diff (`PCIE_DRIVER = 1<<4`, `PLATFORM = 1<<5`, `SUPERVISOR = 1<<6`).

**Verified on:** RV64 QEMU TCG 8.2.2, `harts=1`, kernel `76832e05…`, feature tuple
`native-domains,test-hooks` — raw log `docs/evidence/cap-revoke-qemu.{log,txt}`.

### Delivered
- `libs/api/src/abi/syscall.rs::cap_mask`: the three bits, each documented with the
  teardown it triggers, and `cap_mask::ALL` extended. Strictly additive — no existing bit
  moved, no reader other than the kernel revoke arm exists, and a cell that does not know
  the bits is unaffected.
- Revoke dispatch: `PCIE_DRIVER` → `task::teardown_dma_authority(tid)` (IOMMU domain +
  requester entries + the frames a driver was holding) then `revoke_mmio_classes(DEV_PCIE)`
  then owned-grant reclaim; `PLATFORM` → `revoke_mmio_classes(CLASS_ECAM)`; `SUPERVISOR` →
  field clear (kernel-minted, never propagated, no ambient resource behind it).
- `task::teardown_dma_authority` extracted from the retirement path, so **cell death and a
  runtime `pcie_driver` revoke run one DMA teardown** and a revoked driver Cell ends in
  exactly the hardware state it would reach by dying. The retirement retry policy stays in
  the retirement funnel; the revoke path fails closed instead of queueing a retry.

### Evidence (markers in the published log)
`thread-cap self-test PASS` includes `REVOKE-PRIVILEGED`, which asserts on one call
revoking `PCIE_DRIVER | PLATFORM | SUPERVISOR`: all three TCB fields cleared, the claimed
BDF released (`owner_of_bdf` → `None`), the claimed BAR window released under the
`test-hooks` registration hook, and the notification carrying the new mask.

### Deviations from the plan
- **F3 was vacuous.** The plan said to remove these three from P00's reject-filter; they
  were never in it — P00's own note records that they had no `cap_mask` bit and so were
  not encodable. Nothing to remove; the filter now holds `HYPERVISOR` alone.
- `git diff libs/api` is exactly the three constants plus `ALL`.

### Gate decision (recorded)
`SUPERVISOR` revoke keeps the existing Gate-1 (`SpawnCap`) plus the existing
`target_tid == caller_id` self-revoke guard. No higher gate was added: `supervisor_cap` is
kernel-minted and never propagated, and a supervisor revoking a peer supervisor is the
intended use (that is the authority that can already `ForceExit` it).
