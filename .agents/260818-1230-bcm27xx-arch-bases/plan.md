---
title: "BCM27xx Arch Base Consumption"
description: "Source BCM2837 controller bases from the SoC profile while retaining mechanisms in ARM arch HAL."
status: completed
priority: P2
effort: 4h
branch: fix/structure
tags: [refactor, hal, bcm27xx, aarch64]
created: 2026-08-18
---

# BCM27xx Arch Base Consumption

## Scope Contract

- Add BCM2837 system-timer and legacy-IRQ controller bases to `hal/soc/bcm27xx`.
- Activate an optional BCM27xx SoC-data dependency through `hal-arm/board-rpi3`.
- Replace absolute BCM bases in the mini-UART, legacy IRQ, system timer, local IRQ, and timer diagnostics.
- Preserve every register offset, IRQ number, timer period, pinmux operation, and driver mechanism in `hal/arch/arm`.
- Exclude kernel/board changes, IRQ-topology extraction, feature collapse, and physical runtime claims.

## Phases

| Phase | Name | Status | Depends |
|---|---|---|---|
| 1 | [Extend BCM controller bases](./phase-01-extend-controller-bases.md) | completed | none |
| 2 | [Consume bases in ARM HAL](./phase-02-consume-bases-in-arm-hal.md) | completed | 1 |
| 3 | [Verify, review, and document](./phase-03-verify-review-document.md) | completed | 2 |

## Compatibility Strategy

Only absolute base sources and Cargo feature wiring change. Offsets and all observable register accesses remain numerically identical.

## Deferred Work

- BCM2835/BCM2836 IRQ-number and routing policy extraction.
- Timer-frequency/quantum policy extraction.
- Executable board pinmux generation and physical RPi3 validation.

## Evidence Boundary

QEMU RV64 runtime is regression-tested. RPi3 remains compile-only for this slice.
