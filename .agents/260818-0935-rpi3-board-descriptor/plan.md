---
title: "RPi3 Board Descriptor Slice"
description: "Add an audited Raspberry Pi 3 board descriptor and consume it for platform defaults and fallback memory."
status: completed
priority: P2
effort: 6h
branch: fix/structure
tags: [refactor, boards, aarch64, rpi3]
blockedBy: []
blocks: []
created: 2026-08-18
---

# RPi3 Board Descriptor Slice

## Scope Contract

- Add one root `boards/raspberry-pi/3-model-b` descriptor containing identity, compatibles, VideoCore boot contract, fallback memory, pinmux/PHY labels, and enabled shared drivers.
- Make only PLIC, CLINT, and RTC descriptor entries optional so non-RISC-V boards are represented without fake MMIO ranges; UART remains mandatory.
- Consume the descriptor from RPi3 `kernel/src/board.rs`, `platform.rs`, and `boot.rs` fallback paths.
- Preserve `hal/soc/bcm27xx` ownership of BCM2837 controller layout and SDHCI access policy.
- Exclude paging, IRQ/timer mechanisms, MMC driver logic, feature collapse, DTB parsing, and physical-board runtime claims.

## Phases

| Phase | Name | Status | Depends |
|---|---|---|---|
| 1 | [Generalize optional controller data](./phase-01-generalize-optional-controller-data.md) | completed | none |
| 2 | [Add and consume the RPi3 descriptor](./phase-02-add-and-consume-rpi3-descriptor.md) | completed | 1 |
| 3 | [Verify, review, and document](./phase-03-verify-review-document.md) | completed | 2 |

## Compatibility Strategy

QEMU RV64 keeps identical values wrapped in `Some`. RISC-V fallback consumers fail closed when a required QEMU controller is absent. RPi3 uses `None` for PLIC, CLINT, and RTC rather than sentinel MMIO records.

## Deferred Work

- BCM2837 paging range extraction.
- BCM2835/BCM2836 IRQ and timer policy.
- Executable pinmux generation from descriptor strings.
- Physical RPi3 validation and board-feature collapse.

## Evidence Boundary

QEMU RV64 runtime is verified. RPi3 remains compile-only for this slice.
