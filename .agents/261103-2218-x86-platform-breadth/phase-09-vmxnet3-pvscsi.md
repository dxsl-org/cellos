---
phase: 9
title: "vmxnet3 and PVSCSI cells, VMware matrix"
status: in-progress
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

- [x] V0 VMware tooling smoke: pipe relay, typed input, two-boot persistence on SATA and NVMe (BIOS + EFI, 2/4 vCPU), host probes ([evidence](reports/s3-v0-vmware-tooling-evidence.md), [JSON](reports/s3-v0-vmware-smoke.json))
- [ ] vmxnet3 (QEMU)
- [ ] PVSCSI (QEMU)
- [ ] Matrix script (VMX generation for every cell and the `lane` runner landed in V0; the `nic × storage × vcpu × firmware` sweep with DHCP verdicts remains)
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

- **V0-D1 (Decision, 2026-11-06).** One pinned `virtualHW.version = "22"` (`vmware-boot-test.py` `HW_VERSION`), recorded from `vmware.log` per boot. The AHCI fault scenarios use it too, through the shared generator (14 → 22, not re-run).
- **V0-D2 (Decision).** Lane disk = `machine_pin_disk.py` → `qemu-img` seed VMDK → `vmware-vdiskmanager.exe -r … -t 0`. `x86_64-vm` needs the pin store, so the guest is a provisioned node.
- **V0-D3 (Decision).** PCIe root ports `pciBridge4-7` are always in the VMX. Without them, e1000e/vmxnet3/NVMe fail power-on with `msg.pci.noslotavail`. `pciBridge0` is opt-in (`legacy_bridge`), see V0-S2.
- **V0-D4 (Deviation).** EFI cells run at 512 MiB until V0-S3 is fixed; BIOS cells at 1024 MiB.
- **V0-S1 (Surprise, blocks the VMware NIC rows of 08/09).** VMware puts e1000e/vmxnet3 on bus ≥ 1. The kernel scan logs bus 0 only, and the platform cell skips every class `02:00:00` except `8086:100E` (`cells/services/platform/src/scan.rs:222-229`, outside request §2). The new NIC cells cannot see their VMware devices without a discovery-path change. Owner decision needed.
- **V0-S2 (Surprise).** With `pciBridge0`, AHCI moves to bus 2 and logs `[ahci] controller has no usable MMIO BAR (base=0x0 len=0x2000)`, then SAFE MODE. Recorded by probe `bridge0-sata`.
- **V0-S3 (Surprise).** EFI with ≥ 1024 MiB panics: `kernel/src/main.rs:1219:10: frame allocator bitmap and early paging registration must succeed`. BIOS is unaffected up to 4096 MiB. Kernel fix outside S3.
- **V0-S4 (Surprise).** VMware EFI `\_S5_` is unparsed (`scan stopped at opcode 0x5b offset 0x1f59; poweroff refused`), so EFI guests are hard-stopped. Persistence is unaffected.
- **V0-S5 (Surprise, closes A3).** `vvtd.enable` powers on and exposes a DMAR, but `[vtd] Intel VT-d not present (GCAP=0xffffffffffffffff)`, and `x86_64-pc` refuses untranslated DMA. A3 is false, so H-S3-9 applies: the matrix runs under `x86_64-vm` only, with no VMware production-guest claim.
