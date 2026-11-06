---
phase: 9
title: "vmxnet3 and PVSCSI cells, VMware matrix"
status: "implemented (S3 checkpoint: DMA/ring review pending)"
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
- [x] vmxnet3 (QEMU) — `reports/phase-09-evidence.md`
- [x] PVSCSI (QEMU) — `reports/phase-09-evidence.md`
- [x] Matrix script: `matrix` sweep of `nic × storage × vcpu × firmware` plus one `pciBridge0` cell per storage type, with prompt / NIC + DHCP / persistence / guest-S5 verdicts and 12 unit tests ([evidence](reports/phase-09-evidence.md#vmware-matrix-09-3-09-4))
- [x] VMware matrix run + evidence: 39/39 PASS, run `20261106T013618Z-6528a8` ([JSON](reports/s3-phase09-vmware-matrix.json))

## Success Criteria

- [x] `vmxnet3-x86` (DHCP, ± VT-d) and `pvscsi-x86` (persistence) pass under TCG and KVM, on the integration tree before and after the D-S5/D-S6 fixes: `2` / `3 passed` each. These are local runs of the CI recipe; no CI run was observed from this lane.
- [x] VMware matrix: every cell of {e1000, e1000e, vmxnet3} × {sata, nvme, pvscsi} × {2, 4 vCPU} × {bios, efi} passes: 36/36, plus the 3 `pciBridge0` cells, two boots each, EFI at 1024 MiB. Results JSON committed as local evidence (ceiling: VMware local, `x86_64-vm` only).
- [x] `test_native_vmware_matrix.py` unit tests pass (`Ran 43 tests … OK`).
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
- **V0-D1 (Decision, 2026-11-06).** One pinned `virtualHW.version = "22"` (`vmware-boot-test.py` `HW_VERSION`), recorded from `vmware.log` per boot. The AHCI fault scenarios use it too, through the shared generator (14 → 22, not re-run).
- **V0-D2 (Decision).** Lane disk = `machine_pin_disk.py` → `qemu-img` seed VMDK → `vmware-vdiskmanager.exe -r … -t 0`. `x86_64-vm` needs the pin store, so the guest is a provisioned node.
- **V0-D3 (Decision).** PCIe root ports `pciBridge4-7` are always in the VMX. Without them, e1000e/vmxnet3/NVMe fail power-on with `msg.pci.noslotavail`. `pciBridge0` is opt-in (`legacy_bridge`), see V0-S2.
- **V0-D4 (Deviation).** EFI cells run at 512 MiB until V0-S3 is fixed; BIOS cells at 1024 MiB.
- **V0-S1 (Surprise, blocks the VMware NIC rows of 08/09).** VMware puts e1000e/vmxnet3 on bus ≥ 1. The kernel scan logs bus 0 only, and the platform cell skips every class `02:00:00` except `8086:100E` (`cells/services/platform/src/scan.rs:222-229`, outside request §2). The new NIC cells cannot see their VMware devices without a discovery-path change. Owner decision needed.
- **V0-S2 (Surprise).** With `pciBridge0`, AHCI moves to bus 2 and logs `[ahci] controller has no usable MMIO BAR (base=0x0 len=0x2000)`, then SAFE MODE. Recorded by probe `bridge0-sata`.
- **V0-S3 (Surprise).** EFI with ≥ 1024 MiB panics: `kernel/src/main.rs:1219:10: frame allocator bitmap and early paging registration must succeed`. BIOS is unaffected up to 4096 MiB. Kernel fix outside S3.
- **V0-S4 (Surprise).** VMware EFI `\_S5_` is unparsed (`scan stopped at opcode 0x5b offset 0x1f59; poweroff refused`), so EFI guests are hard-stopped. Persistence is unaffected.
- **V0-S5 (Surprise, closes A3).** `vvtd.enable` powers on and exposes a DMAR, but `[vtd] Intel VT-d not present (GCAP=0xffffffffffffffff)`, and `x86_64-pc` refuses untranslated DMA. A3 is false, so H-S3-9 applies: the matrix runs under `x86_64-vm` only, with no VMware production-guest claim.
- **D-S1 (S1 fix: Ethernet behind bridges and root ports).** Per the [grant](s3-v0-findings-grant.md) and the [S1/S2 report](reports/s3-fix-pci-discovery-evidence.md):
  - The platform cell's hand-kept `100E` filter is gone (`scan.rs`). Every function is offered to the kernel. `register_device` (`pcie_ecam.rs`) applies `ethernet_id_has_driver`, the one Ethernet ID table, and syscall 236 returns `NotSupported` for a refused function.
  - A refusal now prints `[platform] registration refused bus B device D function F id vvvv:dddd class C:S:P`.
  - Deviation: the gate decision needed `pcie_ecam.rs` and `syscall.rs`. Both are S2 rows; this is kernel policy, not wire.
  - Deviation (D-S1b): a refused function's BAR windows stay registered, as in the bus-0 scan. Without the 418/424 hand-out a window grants nothing.
  - Closes V0-S1 and D8.
- **D-S2 (S2 fix: every BAR of a Platform-registered function).** `register_device` decodes every BAR and the MSI-X/PM capabilities through `decode_function_in_window`, as the bus-0 scan does. `offer_pcie_device` uses `first_memory_bar`. Root cause: the 236 path stored only BAR0, and AHCI's ABAR is BAR5. Closes V0-S2. Matrix: `bridge0-sata-e1000-c2-bios` → `[ahci] ABAR claim ok base=0xfd5ef000` on `02:01.0`. New QEMU test `bridged_ahci_and_root_port_nics_follow_the_scan_rules` (`pcie-multibus-x86`).
- **D-S3 (S3 fix: EFI boot heap in the bitmap's range).** Per the [report](reports/s3-fix-s3-evidence.md): the heap was a first fit over all ranges and could land below the bitmap, which sits in the largest range. The `[bitmap, heap)` registration then failed (`main.rs:1219`). `reserve_boot_heap` now reserves inside the bitmap's range and refuses boot otherwise. Closes V0-S3 and supersedes V0-D4: matrix EFI cells run at 1024 MiB, and OVMF 1/2/4 GiB boots. Surprise reported there, not fixed: OVMF `-smp ≥ 2` panics `AP requires permanent shared kernel root`, because a PML4 at frame 0 collides with the "unset" sentinel. VMware maps do not start at 0, so the lanes are unaffected.
- **D-S4 (S4 fix: `\_S5_` scan on VMware's EFI DSDT).** Per the [report](reports/s3-fix-s4-evidence.md): `skip_term` accepts integer-expression TermArgs with a `NullName` target (VMware EFI `OperationRegion (EICH, SystemMemory, (ECFG + 0x4000), 0x4000)`). A storing target is still `Unparsed`. VMware DSDT fixtures were added. Closes V0-S4. Matrix: `\_S5_ static SLP_TYPa=0 SLP_TYPb=0` and guest S5 power-off on all 78 boots, BIOS and EFI.
- **D-S5 Surprise → fix (vmxnet3 under VMware EFI).**
  - Symptom: `[vmxnet3] controller initialization failed: unusable MAC ff:ff:ff:ff:ff:ff` (EFI only). BIOS read the MAC through the same queries.
  - `[INFERENCE]` EFI leaves memory decoding off on the function, and the cell read BAR1 before `GrantDma` enabled `MEM_SPACE | BUS_MASTER`. The command register was not read directly.
  - Fix `6d3192477`: the DMA area is authorized before the first register access, and `negotiate` refuses all-ones `VRRS`/`UVRS`.
  - Smallest option, inside the cell. The other cells also read registers before 435/`GrantDma` and rely on firmware-enabled decoding; this is an open item for S4 (physical).
- **D-S6 Surprise → fix (vmxnet3 DHCP on VMware).**
  - The net service frames with a fixed `52:54:00:12:34:56`, not the NIC's MAC. QEMU's default vNIC MAC is the same address, which hid the problem.
  - The e1000 family receives in promiscuous mode. vmxnet3 filtered unicast to `00:0c:29:…`, so the DHCP OFFER was dropped.
  - Fix `14640724a`: `rxMode` adds PROMISC (`0x1D`). This updates D3's "rx mode unicast+broadcast+all-multicast".
  - The fixed station MAC itself (one MAC for every Cellos guest on an L2) is outside S3 and is reported as an open risk.
- **D-S7 Decision (matrix shape).**
  - The lease is required on both boots, inside 45 s of power-on. Guest S5 is a verdict: no hard stop.
  - `pciBridge0` cells: `sata`+e1000 c2 BIOS, `nvme`+e1000e c4 EFI, `pvscsi`+vmxnet3 c2 EFI.
  - Cells run three at a time (`--jobs 3`); each has its own directory, pipe and disk.
  - The committed JSON drops only the PCI bridge and root-port enumeration lines from the markers; the full summary is retained under `build/` with its sha256.
- **D-S8 Observation (D6 on the real device).** VMware's PVSCSI offers MSI-X: `irq=msix vectors=1 irq_wakes=0..5 park_expiries=0` on 26 boots, with no `irq wait expired` line. D6's post-park re-reads remain a QEMU (MSI) observation. D5 (VT-d confinement of PVSCSI ring pages) stays unproven, because VMware exposes no working remapper (V0-S5).
