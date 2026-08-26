---
phase: 1
title: "Close ARM HAL Consumers"
status: completed
priority: P2
effort: "0.5h"
dependencies: []
tier: medium
---

# Phase 1: Close ARM HAL Consumers

## Overview

Remove the remaining IRQ topology literals in the BCM legacy pending and CNTP routing paths.

## Requirements

- Derive GPIO pending-bank bits from `GPIO_BANK0_IRQ` and `GPIO_BANK1_IRQ`.
- Pass `IRQ_SRC_TIMER_NS` to the Core0 timer-routing mechanism.
- Preserve public constants and all register offsets.

## Architecture

The SoC profile owns numbers and masks; ARM HAL continues to own bank selection and MMIO operations.

## Assumptions

None — target symbols and consumers were read directly.

## Related Files

- Modify: `hal/arch/arm/src/aarch64/bcm2835_legacy_irq.rs`
- Modify: `hal/arch/arm/src/aarch64/timer.rs`

## Implementation Steps

1. Replace GPIO pending literals with masks derived from the existing aliases.
2. Replace the CNTP route literal with the existing local-source alias.

## Success Criteria

- [x] No matching IRQ topology literal remains in the scoped ARM HAL paths.
- [x] AArch64 HAL default and `board-rpi3` checks pass.

## Security Considerations

N/A.

## Risk Notes

An incorrect bank conversion can hide GPIO interrupts. Revert these two substitutions to undo the phase.

## Deviation Log

None.
