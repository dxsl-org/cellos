---
phase: 2
sub: b
title: "AHCI part B — data path, block registration, persistence"
status: pending
priority: P1
effort: M (sub-phase of the AHCI family)
dependencies: [2, 1]
tier: thinking
ceiling: qemu
---

# Phase 02b — AHCI part B: data path, block registration, persistence

## Target

Roadmap gate **X86-PC-1**, second half: make the SATA device usable as storage,
so `/data` and the hypervisor guest disk can live on it.

## Change

- READ/WRITE DMA EXT through the command list built in 02a, with bounded polling
  (interrupts only if the existing IRQ substrate makes it cheap).
- Register as a block device through the **same path NVMe uses**, so VFS,
  littlefs `/data`, and the hypervisor's guest-disk file work unchanged — no
  second block-registration surface.
- Bounded error handling: device fault, task-file error, and timeout must
  surface as typed errors with the port/tag named, never as a silent stall.
- If 02a's polled path proves too slow for the persistence gate, an interrupt
  path is in scope here; otherwise it stays out.

## QEMU-first gate

- Extend `tests/integration/tests/ahci-x86.rs`: block registration marker
  (`[driver_cell] block driver registered`) → FAT32 mount → marker write →
  **second boot on the same image** → marker read back (the NVMe lane already
  proves this two-boot shape).
- Regression set that must stay green: `nvme-x86` 3/3, `pcie-multibus-x86` 2/2,
  `x86_64-boot` 7/7 — the registration ordering these lanes depend on must not
  change.

## Acceptance

- On QEMU: the marker survives a reboot on the same raw image, and the
  regression set above is green.
- Unsupported/absent device and fault paths are typed and logged with port/tag.
- No change to block-registration ordering for NVMe/virtio-blk.

## Out of scope

- Filesystem choice (this sub-phase only makes the block device behave like the
  existing ones), NCQ, hotplug, TRIM, port multipliers.

## Risk assessment

- **Undo:** remove the data-path code and the registration call; 02a's controller
  init remains harmless on its own.
- **Not undoable:** promoting a physical machine's storage to a working HCL row —
  reserved for phase 07.
