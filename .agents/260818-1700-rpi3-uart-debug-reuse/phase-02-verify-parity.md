---
phase: 2
title: "Verify Parity And Regression"
status: completed
priority: P2
effort: "1h"
dependencies: [1]
tier: medium
---

# Phase 2: Verify Parity And Regression

## Overview

Prove the one-file driver reuse does not alter build or boot behavior.

## Requirements

- Pass formatting and the established host/AArch64/RV64 matrix.
- Pass a scoped guard against the removed raw UART implementation.
- Preserve the RV64 QEMU FAT16 witness.

## Architecture

Compile gates prove cfg visibility; the grep guard proves duplication removal; QEMU protects the unaffected runtime lane.

## Assumptions

None — validation commands are established in the preceding slices.

## Related Files

- Read: `kernel/src/task.rs`
- Read: `hal/arch/arm/src/aarch64/uart_bcm_mini.rs`

## Implementation Steps

1. Capture baseline before the source edit.
2. Run focused AArch64 checks after the edit.
3. Run the complete 11-gate matrix and literal guard.

## Success Criteria

- [x] Baseline and final matrices pass 11/11.
- [x] Scoped raw-UART duplication guard passes.

## Security Considerations

N/A.

## Risk Notes

Compile and RV64 runtime evidence do not prove RPi3 serial timing. Keep physical validation deferred.

## Deviation Log

None.
