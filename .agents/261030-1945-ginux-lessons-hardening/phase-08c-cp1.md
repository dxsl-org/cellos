# Phase 08C — CP1 (AHCI restart through a documented reset, fresh lease, readback)

Status: PROPOSED, awaiting user decision. No source edited.

## Current state

- Block drivers are spawned once by init (`cells/tools/init/src/boot.rs:434-442`)
  and are not supervised. A dead `/bin/ahci` stays dead, and storage is gone
  until reboot.
- 08B made retirement safe to re-grant from:
  1. the function is quiesced (Bus Master off, INTx off);
  2. the IOMMU is revoked (where present);
  3. only after both does the BDF become claimable again.
  If either step fails, the BDF and frames are retained.
- `AhciController::new` already performs the AHCI 1.3.1 §10.4.3 HBA reset
  (`GHC.HR`, self-clearing, bounded poll) before port bring-up
  (`controller.rs:209-222`). This is the documented controller-scope reset;
  FLR is not needed. ICH9 AHCI is a single function with no sibling owner.
- VFS already re-probes the block driver service after a failed exchange
  (`blk_router::forget_driver`).

## Proposal (08C)

1. **Restart policy (init).** `/bin/ahci` becomes restartable with the
   existing service crash budget. When it exits, init respawns it.
   - The new incarnation's BDF claim succeeds only once 08B has released the
     BDF, which is the "fresh lease".
   - While the BDF is retained (quiescence or IOMMU acknowledgement failed),
     the claim fails and the replacement exits; init's budget then gives up.
     The device stays quarantined; it is never reused.
   - The NVMe and virtio-blk drivers are unchanged.
2. **Reset proof (driver).** The replacement logs the HBA reset completion
   and IDENTIFY before it re-registers `BLOCK_DRIVER`. A reset that never
   completes is a bring-up failure (`exit 1`), never a registration.
3. **Readback witness** (fault ISO):
   - write a committed file;
   - `F08B-DIE`;
   - observe quiesce, then respawn, `HBA reset complete` and re-registration;
   - `vcat` the committed file through the new incarnation.
   The kernel should log the new tid claiming 00:1f.2.
4. **Non-reuse witness** (kernel host test): a teardown whose quiescence
   fails leaves `owner_of_bdf` set to the dead tid, and `claim_bdf_owner` by a
   new tid returns `false`. An injected IOMMU acknowledgement failure on QEMU
   is not reachable on q35 (no VT-d on that lane), so it stays at host-test
   level. This is stated as a residual.

Out of scope:
- NVMe/virtio restart;
- a VFS-level write journal (writes left Indeterminate when the driver died
  stay Indeterminate and are surfaced as `EIO`);
- physical hardware.
