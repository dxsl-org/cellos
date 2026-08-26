---
title: "Complete Cellos HAL Split"
description: "Finish descriptor, SoC-data, shared-driver, and build-selection boundaries for current boards."
status: completed
priority: P1
effort: 3d
branch: fix/structure
tags: [architecture, boards, hal, soc, drivers]
blockedBy: []
blocks: []
created: 2026-08-18
---

# Complete Cellos HAL Split

## Completion Contract

- Every current board feature (`board-vf2`, `board-pioneer`, `board-rpi3`, `board-rpi4`) and both default QEMU machines has one audited root-board descriptor and rebuild command.
- Board packages contain identity/compatibles, boot contract, wiring, fallback memory/DT asset, and typed enabled-driver selection only.
- Immutable MMIO/IRQ/access quirks live under `hal/soc`; mechanisms live under `hal/arch` or the single shared kernel/Driver Cell implementation.
- UART, SDHCI, GIC/PLIC, VirtIO, and PCIe mechanisms are not copied per board.
- Existing feature names and public ABI remain compatible; physical boards remain compile-only unless real hardware is exercised.

## Phases

| Phase | Name | Status | Depends |
|---|---|---|---|
| 1 | [Complete the typed board catalog](./phase-01-complete-board-catalog.md) | completed | none |
| 2 | [Make RISC-V selection descriptor-driven](./phase-02-riscv-descriptor-selection.md) | completed | 1 |
| 3 | [Extract QEMU ARM virt SoC facts](./phase-03-arm-virt-soc-profile.md) | completed | 1 |
| 4 | [Make shared SDHCI policy data-driven](./phase-04-sdhci-runtime-policy.md) | completed | 1, 2 |
| 5 | [Drive build selection from typed board data](./phase-05-driver-build-selection.md) | completed | 2, 3, 4 |
| 6 | [Enforce boundaries and close documentation](./phase-06-boundary-closure.md) | completed | 5 |

## Compatibility Strategy

Migrate one consumer family at a time, preserve feature names, and commit only after the baseline-relative matrix passes. Runtime DTB remains authoritative; descriptors remain audited fallback/build data.

## Evidence Boundary

RV64 FAT16 boot and AArch64 `ViCell >` are the QEMU runtime regression
witnesses. VF2, Pioneer, RPi3, and RPi4 are compile-only without matching
physical runs.
