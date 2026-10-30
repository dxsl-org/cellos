---
phase: 8
title: "Ordered device quiescence"
status: pending
priority: P1
effort: "1.5w"
dependencies: [4, 7]
tier: thinking
---

# Phase 08: Ordered device quiescence

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Overview

Cellos already splits generic IPC timeouts into `PRE_DISPATCH_TIMEOUT` and `INDETERMINATE` (`async_ipc.rs:524-542`). Its retirement funnel is IOMMU revoke → BDF release → frame release (`task.rs:1805-1824`). It never disables interrupt delivery, never clears bus mastering (only `enable_bus_master` exists, `pcie_ecam.rs:688-728`) and never resets the device. This phase adds the ginux ordering (`/tmp/ginux/specs/kernel/device-dma-lease-v0.md:23-37`) kernel-side, plus the prepared/notified distinction in an **x86 PCI** driver Cell, without public ABI.

The existing virtio-blk/virtio-net Cells are VirtIO-MMIO only and enumerate nothing on x86 (`cells/drivers/virtio-blk/src/device.rs:139-165`), so they are not the oracle.

## Requirements

- Kernel retirement order for every BDF owned by the retiring Cell:
  1. Admission cut (existing).
  2. **Interrupt quiescence, all modes:** set Command.INTx-Disable; for every MSI and MSI-X capability present, clear enable and set the MSI-X function mask, regardless of which one the driver used; read back each control register. Synchronize already-routed vectors: wait for any in-flight handler on all CPUs, then drop pending entries for that BDF before releasing routing ownership.
  3. Clear `CMD_BUS_MASTER` and read it back.
  4. FLR if PCIe DevCap.FLR is set; wait 100 ms; confirm the vendor ID reads non-`0xFFFF`. Without FLR, the BDF becomes `quarantined-until-reset` and is not re-grantable until cold boot (fail closed).
  5. IOMMU context clear + IOTLB flush (existing; retained-on-failure semantics kept).
  6. Quarantine acknowledgement / frame release (existing), only after step 5 completes.
  7. BDF release.
- Capability data for steps 2 and 4 comes from a **bounded, range-checked ECAM capability walk at retirement time**, not from cached enumeration metadata. `register_device` stubs carry `msix: None` / `pm: None` (`pcie_ecam.rs:850-934`). No Platform Cell registration ABI change.
- Each step emits a test-hook marker with the BDF and step number. A step failure retains authority and frames (existing retry) and logs the step.
- Driver side, in **AHCI** (x86-verified: `cells/drivers/ahci/src/main.rs`, two-boot persistence evidence) and, if Assumption 2 holds, **NVMe**: per-command state `Prepared` (slot built, not issued) vs `Notified` (PxCI / doorbell written). A prepared timeout retries in the same generation. A notified timeout exits through the existing abnormal-exit path, which the supervisor treats as restart-required. Clients of dispatched operations get the existing `INDETERMINATE`.
- The new generation gets a fresh DMA lease and re-initializes the controller; it never inherits port or queue state.
- e1000/igb/xhci inherit the kernel ordering automatically. Their driver-side tracking is out of scope and listed in Spec 04.

## Assumptions

- **Claim:** the supervisor restart path needs no new public exit code. **Confidence:** medium. **How to verify:** read the `NotifyOnExit` payload in `cells/tools/init/src/supervisor.rs`. If a new code is required, stop: that is a Law-1 change.
- **Claim:** the NVMe Cell runs on x86 q35 with `-device nvme`, and QEMU's nvme model advertises FLR. **Confidence:** low. **How to verify:** boot the NVMe Cell on q35 and dump DevCap via the retirement-time walk. If either fails, the FLR re-grant path stays **unproven** and is reported as such. AHCI (ICH9, expected no FLR) proves the fail-closed path.
- **Claim:** the IRQ routing table permits a per-BDF "drop pending" operation. **Confidence:** medium. **How to verify:** read the x86 MSI/IOAPIC routing in `hal/arch/x86` and `kernel/src/task/drivers`.

## Related Files

- Modify: `kernel/src/task/drivers/{pcie_ecam.rs,iommu_x86.rs,iommu.rs}`, `kernel/src/task.rs` (retirement funnel), `kernel/src/resource_registry.rs` (`quarantined-until-reset`), x86 IRQ routing (per step 2), `kernel/src/memory/pin.rs` (order assertion), `cells/drivers/ahci/src/**`, `cells/drivers/nvme/src/**` (conditional).
- Tests: a test hook in AHCI (and NVMe) that issues a command and never polls completion; a variant that delays before issue (the prepared case); an x86 runner step on the test-hooks lifecycle image from Phase 07.
- Docs: `docs/specs/04-hardware.md` (retirement order, driver coverage), Spec 25 annex (device mapping of INDETERMINATE), `scripts/unsafe-allowlist.toml` (only for reviewed driver unsafe changes).

## Implementation Steps

1. Retirement-time capability walk plus helpers: `disable_intx`, `quiesce_msi_all`, `disable_bus_master`, `flr_if_supported`, each with readback.
2. IRQ sync/drop per BDF.
3. Reorder the funnel; add `quarantined-until-reset`; make grant refuse such BDFs.
4. AHCI request state and timeout split; NVMe if Assumption 2 holds.
5. Test hooks plus runner: a notified stall → ordered markers 2→7 → supervisor restart → a new generation is refused the AHCI BDF (no FLR, marker) / NVMe re-inits and reads a known LBA (FLR).
6. Negative controls: a prepared delay retries without exit; for interrupt modes, run AHCI with MSI and with INTx forced (QEMU `-global ich9-ahci.msi=off` or a driver flag) and check the markers show both disabled.

## Success Criteria

- [ ] The x86 runner shows the ordered markers for a notified stall on AHCI. The outcome matches the no-FLR branch (BDF refused after restart).
- [ ] The NVMe FLR branch passes, or is explicitly reported as unproven with the failing assumption.
- [ ] A prepared timeout retries in the same generation (no exit marker).
- [ ] INTx-only and MSI runs both show interrupt quiescence with readback.
- [ ] `bash scripts/qemu-tier3-hostile-runner-x86.sh build/vicell-x86-virtio-hostile.iso` and the lifecycle test still pass.
- [ ] Test-hooks assertion: no frame leaves quarantine before the IOTLB flush completes.

## Security Considerations

This closes interrupt/DMA activity during teardown and stops device state leaking into the next generation. Fail-closed (no FLR → no re-grant) trades availability for safety: for AHCI on this lane, a driver fault means a node reboot for that controller, and that is documented in Spec 04.

## Risk Notes

AHCI's no-re-grant behavior reduces availability compared with today; the maintainer must accept it in Phase 01 (listed as decision D-08). Physical FLR timing is P10 scope. Undo: revert the funnel order; BDFs quarantined in a test run need a cold boot.

## Deviation Log

None.
