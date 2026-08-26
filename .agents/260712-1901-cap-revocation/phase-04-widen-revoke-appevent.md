# Phase 04 (G2) — Widen `sys_cap_revoke` to dispatch Class-2 teardown + `AppEvent::CapRevoked`

## Context Links
- Plan: [plan.md](plan.md) | Depends on: **P01, P02, P03** (all teardown primitives). Follows P00.
- Design authority: dossier §"Class 2 … must be EAGER" + §"Victim notification — needs an AppEvent".

## Overview
- **Priority:** P1. **Speed:** G2. **Status:** pending. **Effort:** M.
- **Law 1:** none to `libs/api`. **Law-1-adjacent:** adds an `AppEvent` variant in `libs/ostd` (Cellos std, NOT the frozen ABI) AND reserves a new byte-1 envelope discriminant `0xF2` in the IPC wire contract (`spec/17`). Flag both.
- Replace P00's `NotSupported` rejection for the **MMIO** bits with real eager teardown (the only Class-2 surface encodable without new ABI), and notify the victim cell so it can shut a subsystem down gracefully instead of faulting.

## Key Insights
- `sys_cap_revoke` arm (`syscall.rs:1420-1474`) already: gates on `SpawnCap` (Gate-1), refuses system cells holding block_io/network (Gate-2), clears Class-1 fields. This phase adds a **teardown dispatch** before clearing the `mmio_devices` mask, then removes P00's MMIO reject.
- Teardown building blocks now exist: `resource_registry::revoke_mmio_for` + arch unmap (P03), `reclaim_owned_grants` (P02), `iommu::revoke_dma_for_cell` (P01, used by P05 for pcie).
- `AppEvent` (`libs/ostd/src/app.rs:64`) is `#[non_exhaustive]` with a documented "add a wildcard arm" contract → adding `CapRevoked { mask: u32 }` is source-compatible for cells.
- Envelope decode (`app.rs:290-322`): byte 0 = `0xAC` magic, byte 1 = event type. Used: `0x00` Message, `0xFF` Shutdown, `0xF0`/`0xF1` hotswap. **`0xF2` is free** — reserve for CapRevoked, payload `[0xAC, 0xF2, mask_le4]` = 6 bytes. Registry: `spec/17` §3 line ~75.
- Kernel-side event send pattern exists: `hotswap.rs:191 send_snapshot_event` builds `[0xAC, 0xF0, ...]` and IPC-sends. Mirror it for `[0xAC, 0xF2, mask]`.

## Requirements
- **F1:** on `cap_mask & MMIO_MASK != 0`: call `revoke_mmio_for(cell, revoked_class)` + arch page-table unmap, THEN clear `task.mmio_devices &= !mmio_revoke`. Remove P00's MMIO `NotSupported`.
- **F2:** on any Class-2 revoke: call `reclaim_owned_grants(target_tid)` (P02).
- **F3:** after teardown succeeds, send `AppEvent::CapRevoked{mask}` to the target via envelope `[0xAC,0xF2,mask_le4]` (best-effort; a dead/full-queue target does not fail the revoke).
- **F4:** ostd decodes `0xF2` → `AppEvent::CapRevoked{mask}`; `run_with_lifecycle` delivers it.
- **F5:** teardown ordering — tear down (fail-closed) BEFORE clearing the TCB field, so a mid-teardown failure leaves the cap present (no half-revoked state); notification is last.
- **NF1:** revoke remains synchronous + non-blocking (mirror ForceExit: no `yield_cpu`).

## Architecture
Revoke arm becomes: Gate-1 → Gate-2 → **[NEW] Class-2 teardown dispatch** (MMIO unmap + grant reclaim) → clear surviving TCB cap fields/masks → audit `CapRevoked` → **[NEW] notify victim**. HYPERVISOR + privileged (pcie/platform/supervisor) still rejected here (P05 handles the latter after adding ABI bits).

Data flow: caller mask → gate → for each Class-2 bit dispatch its teardown primitive → clear TCB → audit → `[0xAC,0xF2,mask]` IPC to target → ostd `AppEvent::CapRevoked` → cell's handler drops the subsystem.

## Related Code Files
- **Modify:** `kernel/src/task/syscall.rs` (CapRevoke arm — teardown dispatch, remove P00 MMIO reject, add notify; reuse `send_*_event` pattern from `hotswap.rs:191`).
- **Modify:** `libs/ostd/src/app.rs` (add `AppEvent::CapRevoked{mask}` :64; decode `0xF2` :322).
- **Modify:** `docs/specs/17-ipc-wire-contract.md` (§3 discriminant registry — reserve `0xF2` under `0xAC`).
- **Read-only:** P01/P02/P03 primitives.

## Implementation Steps
1. In the CapRevoke arm, add Class-2 dispatch: MMIO bits → `revoke_mmio_for` + arch unmap; then `reclaim_owned_grants(target)`.
2. Remove P00's MMIO branch of the reject-filter (keep HYPERVISOR + privileged rejects).
3. Order: teardown → clear TCB fields → audit → notify. On teardown error, return the error WITHOUT clearing the field.
4. Add kernel `send_cap_revoked_event(tid, mask)` (mirror `send_snapshot_event`).
5. ostd: `AppEvent::CapRevoked{mask}` variant + `0xF2` decode arm (`buf.len() >= 6`).
6. spec/17: add `0xF2 CapRevoked` row to the byte-1 sub-type list.

## Todo List
- [ ] Class-2 teardown dispatch (MMIO + grants) in revoke arm
- [ ] Remove P00 MMIO reject; keep HYPERVISOR/privileged reject
- [ ] Teardown-before-clear ordering + fail-closed on error
- [ ] `send_cap_revoked_event` kernel-side
- [ ] `AppEvent::CapRevoked` + `0xF2` decode in ostd
- [ ] spec/17 discriminant registry row
- [ ] Test: revoke MMIO from a cell → region unmapped + `AppEvent::CapRevoked` received + next MMIO access faults

## Success Criteria
- Revoking an `mmio_devices` bit now returns `Ok` AND the region is torn down (P03 oracle) AND the target receives `AppEvent::CapRevoked{mask}` with the correct mask.
- A teardown failure returns an error and leaves the cap present (observable: `mmio_devices` unchanged).
- 3-arch boot green; a new revoke integration test (cell grabs GPIO MMIO, supervisor revokes, cell faults/handles event) passes.

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|-----------|
| `0xF2` collides with a future/legacy raw protocol | Low×Med | spec/17 §3 mandates the `0xAC` namespace guard; `0xF2` is registered, not hoped-free. |
| Victim never handles `CapRevoked` (old cell, `_ => {}`) | Med×Low | `#[non_exhaustive]` + wildcard contract; teardown already fail-closed regardless of handling. |
| Notify send blocks/fails on full queue | Low×Med | Best-effort `try_send`; revoke does not depend on delivery (teardown already done). |
| Half-revoke on teardown error | Low×High | F5 ordering: never clear TCB field until teardown returns Ok. |

## Security Considerations
Notification is a courtesy, not a security boundary — the teardown (P01-P03) is what enforces revocation; a cell that ignores `CapRevoked` still loses the hardware. Ordering teardown-before-clear prevents a window where the TCB says "revoked" but the resource is still mapped.

## Next Steps
- P05 extends this dispatch to `pcie_driver`/`platform`/`supervisor` once their ABI mask bits exist.
- Rollback: re-instate P00's MMIO reject and drop the dispatch; `AppEvent`/spec changes are additive and harmless if unused.
