---
phase: 2
title: "Close Kernel Diagnostic Consumers"
status: completed
priority: P2
effort: "0.5h"
dependencies: [1]
tier: medium
---

# Phase 2: Close Kernel Diagnostic Consumers

## Overview

Make the RPi3 post-enable diagnostic read the same SoC topology as ARM HAL.

## Requirements

- Source local-controller and legacy-IRQ bases from `BCM2837.mmio`.
- Source GPU and system-timer masks from `BCM2837.irq`.
- Preserve output format `K<gpu><timer><nibble>` and offsets `0x60`/`0x04`.

## Architecture

Kernel diagnostic code owns observation and formatting; SoC data owns immutable addresses and masks.

## Assumptions

None — the diagnostic block and profile fields were read directly.

## Related Files

- Modify: `kernel/src/main.rs`

## Implementation Steps

1. Bind the BCM2837 profile once inside the diagnostic block.
2. Derive the two status-register addresses and masks from it.

## Success Criteria

- [x] Diagnostic bytes and read offsets are unchanged.
- [x] Kernel AArch64 default and `board-rpi3` checks pass.

## Security Considerations

N/A.

## Risk Notes

This code executes after IRQ enable; avoid adding allocation, locking, or logging. Revert the diagnostic substitutions to undo the phase.

## Deviation Log

None.
