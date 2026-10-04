---
phase: 2
sub: a
title: "AHCI part A — PCI binding, HBA init, IDENTIFY"
status: pending
priority: P1
effort: M (sub-phase of the AHCI family)
dependencies: [1]
tier: thinking
ceiling: qemu
---

# Phase 02a — AHCI part A: PCI binding, HBA init, IDENTIFY

## Target

Roadmap gate **X86-PC-1**, first half. `grep -ri ahci` over the Cellos source
(`kernel/src/task`, `cells/drivers`, `hal/soc`, `boards`) has no hits; matches
exist only inside the embedded guest Linux artifacts. Without a storage driver an
industrial SATA-only PC has no persistent `/data` and no persistent guest disk.

## Change

- New Driver Cell `cells/drivers/ahci/` on the `nvme`/`e1000` pattern: Tier 1
  `#![forbid(unsafe_code)]`, `request_mmio` BAR handoff, every DMA buffer through
  `DmaBuf::authorize` so phase 05's IOMMU work applies without rework.
- PCI binding: class `0x01`/`0x06`, prog-if `0x01` (AHCI). Any other prog-if
  (RST/RAID-only firmware mode) **fails closed** with a log line naming the
  prog-if — this is what gives the HCL "SATA in AHCI mode" row teeth.
- AHCI 1.3.1 part A scope: HBA reset, global/port enable, command list +
  FIS receive area allocation, port link spin-up detection, IDENTIFY DEVICE, and
  a bounded polled completion path. No data transfer in this sub-phase.
- Descriptor wiring (verified pre-Build): `DriverId` variant for AHCI storage in
  `boards/src/descriptor.rs` + a `boards/src/catalog_tests.rs` row. Phase 01
  declares the variant so this phase owns disjoint files.
- Packaging: extend `scripts/build-x86_64-cells.ps1` to build the cell and assert
  it is present in the FAT image (pattern from the e1000 DHCP gate,
  `.agents/260903-x86-e1000-dhcp/plan.md`).

## QEMU-first gate

- New `QemuRunner` constructor `boot_x86_bios_with_sata(iso, sata_disk)`:
  q35's built-in ICH9 AHCI with a raw image
  (`-drive file=<img>,if=none,id=sata0,format=raw -device ide-hd,drive=sata0`).
  Verified in the premise smoke: the controller enumerates as `8086:2922`
  class `01:06:01`.
- New `tests/integration/tests/ahci-x86.rs` (modelled on `nvme-x86.rs` and
  `nic-x86.rs`): assert the cell binds the controller, the HBA/port come up, and
  IDENTIFY reports a device. Add the SATA options to
  `scripts/qemu-x86_64-test.sh` so the lane is runnable without the Rust harness.
- Record the QEMU-model caveat: ICH9 AHCI is register-compatible; port count,
  remap, and NCQ behaviour are not validated here (`A-02`).

## Acceptance

- On QEMU: the driver cell registers the controller and logs IDENTIFY success;
  `x86_64-boot` stays 7/7 and `nvme-x86` stays 3/3.
- A SATA controller in a non-AHCI prog-if fails closed with a named reason.
- No new unsafe island; the unsafe ratchet stays satisfied.

## Out of scope

- Data transfer, block registration, filesystem (that is 02b).
- NVMe (shipped), port multipliers, hotplug, TRIM/NCQ tuning, RAID, SAS/HBA.

## Risk assessment

- **Undo:** delete the cell + its packaging entry + the test; the embedded VIFS1
  boot path is untouched, so the system returns to today's behaviour.
- **Not undoable:** any claim that a physical machine's storage works — phase 07.
