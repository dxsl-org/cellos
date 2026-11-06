---
phase: 9
title: "vmxnet3 and PVSCSI cells, VMware matrix"
status: "in progress: 09.1-09.2 done at QEMU; 09.3-4 VMware matrix pending"
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

- [x] vmxnet3 (QEMU) — `reports/phase-09-evidence.md`
- [x] PVSCSI (QEMU) — `reports/phase-09-evidence.md`
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

- **D1 Deviation (file split).** Inside the `cells/drivers/{vmxnet3,pvscsi}/**` (C) rows, to keep every file under 200 lines: vmxnet3 also has `controller.rs` (bring-up), `datapath.rs` (TX/RX), `dma.rs` (the only `unsafe`) and test-only `{regs,rings,shared}_tests.rs`; PVSCSI also has `dma.rs` (the only `unsafe`), `error.rs`, `proto.rs` (request/completion codec), `queue.rs` (`impl ScsiExecutor`) and test-only `proto_tests.rs`. Reversible: merge files.
- **D2 Decision (NOTICE).** H-S3-14 requires attribution for copied tables, so `NOTICE` gains one section for FreeBSD `if_vmxreg.h` (ISC, Tsubai Masanari) and `pvscsi.h` (BSD-2-Clause, VMware), revision `cf59649539e6`. `NOTICE` is not a §2 row; it is the H-S3-14 record. QEMU/Linux sources were read as behaviour references only (no code copied).
- **D3 Decision (vmxnet3 configuration).** One TX (32) and one RX queue (ring 0: 32 head buffers of 2048 B, 31 posted so producer == consumer means empty; ring 1 allocated, never posted; completion ring 64); `automask = 0` (no re-arm needed); 3 MSI-X vectors TX/RX/event with the RX vector masked (request-driven RX), fewer vectors fold onto the last; polled fallback sets `ictrl = DISABLE_ALL`. Guest-OS word FreeBSD/64-bit (the value OpenBSD's driver uses); VLAN ID 0 admitted; rx mode unicast+broadcast+all-multicast. Commands are FreeBSD's `ENABLE`/`SET_RXMODE`/`SET_FILTER`/`VLAN_FILTER` = the plan's `ACTIVATE_DEV`/`UPDATE_RX_MODE`/`UPDATE_MAC_FILTERS`/`UPDATE_VLAN_FILTERS`.
- **D4 Surprise (QEMU PVSCSI `dataLen`).** QEMU's model reports a short transfer as `BTSTAT_DATARUN` but never fills `cmp.dataLen`. The decoder treats `dataLen = 0` on DATARUN/UNDERRUN as "amount unknown" (residual 0; the bounce buffer is zeroed before every data-in command, and `scsi-block` parses by its own length fields); a reported length above the request is a malformed completion (error, poisons the adapter).
- **D5 Surprise (QEMU PVSCSI bypasses the IOMMU for rings).** QEMU reads request descriptors and writes completion descriptors and sense with `cpu_physical_memory_*`, not `pci_dma_*`; only the ring-state page and data use DMA translation. The VT-d lanes pass because Cellos maps IOVA == PA; they do not prove VT-d confinement of the PVSCSI ring pages. The real VMware device is the reference for that (09.3-4).
- **D6 Surprise (PVSCSI interrupt mode).** QEMU's `pvscsi` offers MSI with one vector and no MSI-X, so the lanes log `irq=msi vectors=1`. Under TCG every probe completion arrived inside the 64-read spin (`irq_wakes=0` at bind); under KVM parks were woken by the MSI (`irq_wakes=1..7` at bind). After bind both runs logged `[pvscsi] irq wait expired after 10 ticks; re-reading the ring` (TCG 10 lines, KVM 2 lines, over 3 PVSCSI lanes × 2 boots): some VFS-I/O completions were found by the post-park re-read, not by a wake. Bounded and correct per H-S3-5, but the cause (late MSI vs. a slow host write) is not yet established — open item for the H-S3-13 review.
- **D8 Observation (bus ≥ 1, integrator request).** QEMU probe with `vmxnet3` and `pvscsi` each behind a `pcie-root-port`: the platform cell logs `[platform] unsupported Ethernet identity — registration skipped` for the vmxnet3 at bus 1 and `/bin/vmxnet3` exits `no vmxnet3 controller present; driver cell idle`; `pvscsi` registers at `bus 2 device 0` and binds (`[pvscsi] pci 02:00.0 15ad:07c0 … irq=msi`). `cells/services/platform/src/scan.rs` not edited (owner decision pending).
- **D7 Decision (PVSCSI target scan).** Targets 0..16 (VMware's default bus width; QEMU allows 64), selection timeout = absent target, first disk wins; command status of `ADAPTER_RESET`/`SETUP_RINGS` and the device-written ring sizes are checked before use.
