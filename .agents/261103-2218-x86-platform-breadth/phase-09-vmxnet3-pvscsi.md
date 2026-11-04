---
phase: 9
title: "vmxnet3 and PVSCSI cells, VMware matrix"
status: pending
priority: P2
effort: "5d"
dependencies: [5, 8]
tier: medium
checkpoint: true
---

# Phase 09: vmxnet3, PVSCSI and the VMware matrix

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- `scripts/x86/vmware-boot-test.py:109-113` (`ethernet0`/`sata0`/`scsi0` disabled today), `scripts/x86/test-native-vmware-matrix.py:73-83` (ethernet off, SATA optional), `scripts/x86/test_native_vmware_matrix.py`
- Phase 05 `libs/scsi-block`; QEMU `vmxnet3` and `pvscsi` models (`hw/net/vmxnet3.c`, `hw/scsi/vmw_pvscsi.c`) as register references
- Linux `drivers/net/vmxnet3/vmxnet3_defs.h`, `drivers/scsi/vmw_pvscsi.h` (reference only; no code copy — GPL)

## Overview

The two paravirtual devices VMware recommends: vmxnet3 `15AD:07B0` and PVSCSI `15AD:07C0`, plus a VMware matrix that exercises every supported NIC × storage combination. Closes S3.

## Key Insights

- Both devices have QEMU models, so most of the work is gated in CI. VMware only adds independent-implementation evidence (local lane).
- The PVSCSI request path reuses `libs/scsi-block`; only the ring transport is new.
- License: Linux headers are GPL. Implement from the device behaviour and VMware's public specs. Any copied constant table needs a LICENSING.md check.

## Requirements

- Functional:
  - vmxnet3 cell:
    - BAR0 (PT: doorbells) + BAR1 (VD: commands).
    - Version negotiation (`VRRS`/`UVRS` = 1).
    - `DriverShared` + queue descriptor area via `GrantDma`; 1 Tx + 1 Rx queue.
    - Commands `ACTIVATE_DEV`, `GET_LINK`, `UPDATE_MAC_FILTERS`; MSI-X (one vector per queue + event).
    - Net-bridge registration.
  - PVSCSI cell: BAR0, rings state + req/cmp rings via `GrantDma`, `SETUP_RINGS`, kick registers, MSI/MSI-X, first-disk block route via `libs/scsi-block`.
  - VMware matrix script:
    - NIC ∈ {e1000, e1000e, vmxnet3}; storage ∈ {sata (AHCI), nvme, pvscsi}; vCPU ∈ {2, 4}; firmware ∈ {bios, efi}.
    - Judged on `Cellos >`, DHCP lease, two-boot persistence.
    - One variant with vIOMMU under `x86_64-pc` if A3 holds; otherwise `x86_64-vm`.
    - Results written as JSON evidence.
  - `DriverId::{Vmxnet3, Pvscsi}`; image + CI for the QEMU lanes.
- Non-functional: none beyond the matrix.

## Architecture

`cells/drivers/vmxnet3/src/{main,regs,shared,rings,irq}.rs`; `cells/drivers/pvscsi/src/{main,regs,rings,irq}.rs` → `libs/scsi-block`.

## Assumptions

- **Claim (plan A3):** VMware Workstation's "Virtualize IOMMU" exposes a DMAR table to the guest. **Confidence:** low. **How to verify:** boot with the vmx key set and read `[acpi] gates: dmar=`.
- **Claim:** VMware Workstation Pro is installed on the developer host used for the existing VMware lane. **Confidence:** medium. **How to verify:** `scripts/x86/vmware-boot-test.py --help` + `find_vmrun`.
- **Claim:** VMware's virtual platform exposes HPET and MCFG. **Confidence:** medium. **How to verify:** boot log gates line.

## Related Code Files

- Create: `cells/drivers/{vmxnet3,pvscsi}/**`, `tests/integration/tests/{vmxnet3,pvscsi}-x86.rs`
- Modify: `scripts/x86/test-native-vmware-matrix.py`, `scripts/x86/test_native_vmware_matrix.py`, `scripts/x86/vmware-boot-test.py`, `tests/integration/src/lib.rs`, `boards/**`, ISO scripts, CI job

## Implementation Steps

1. vmxnet3 against QEMU `-device vmxnet3`: link, then DHCP; VT-d variant.
2. PVSCSI against QEMU `-device pvscsi -device scsi-hd`: two-boot persistence.
3. Extend the VMware matrix script (vmx keys: `ethernet0.virtualDev`, `scsi0.virtualDev = "pvscsi"`, `nvme0.present`, `sata0.present`, `numvcpus`, `firmware = "efi"`, vIOMMU key). Its own unit test covers vmx generation.
4. Run the matrix locally; store the JSON evidence under the plan's `reports/`.

## Todo List

- [ ] vmxnet3 (QEMU)
- [ ] PVSCSI (QEMU)
- [ ] Matrix script
- [ ] VMware matrix run + evidence

## Success Criteria

- [ ] `vmxnet3-x86` (DHCP, ± VT-d) and `pvscsi-x86` (persistence) pass in CI under TCG and locally under KVM.
- [ ] VMware matrix: every cell of {e1000, e1000e, vmxnet3} × {sata, nvme, pvscsi} × {2, 4 vCPU} × {bios, efi} passes. Results JSON committed as local evidence.
- [ ] `test_native_vmware_matrix.py` unit tests pass.
- [ ] Review pass on both cells' DMA/ring code.

## Risk Assessment

- QEMU's vmxnet3/pvscsi models are less exercised than VMware's real devices; divergences surface only in the local VMware lane. Record each one as a Surprise.
- EFI boot under VMware with Limine: if it fails, BIOS-only is recorded and EFI is investigated as a deviation, not dropped silently.

## Security Considerations

Both devices DMA through the shared-memory areas, so every area goes through `GrantDma` and is covered by the IOMMU when one is present.

## Next Steps

S4 (physical platform).

## Deviation Log

None.
