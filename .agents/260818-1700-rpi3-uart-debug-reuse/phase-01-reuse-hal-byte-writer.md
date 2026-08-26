---
phase: 1
title: "Reuse The ARM HAL Byte Writer"
status: completed
priority: P2
effort: "0.25h"
dependencies: []
tier: medium
---

# Phase 1: Reuse The ARM HAL Byte Writer

## Overview

Remove a private copy of mini-UART FIFO polling and byte emission from kernel task setup.

## Requirements

- Keep the exact `aarch64 + board-rpi3` cfg gate.
- Route every debug byte through `crate::hal::uart_bcm_mini::probe_put`.
- Keep `fifo_hex`, labels `1` through `4`, newlines, and the volatile `sepc` read unchanged.

## Architecture

ARM HAL owns mini-UART MMIO and FIFO readiness. Kernel code owns which diagnostic bytes are emitted.

## Assumptions

None — the helper visibility, cfg gate, and matching FIFO-safe behavior were read directly.

## Related Files

- Modify: `kernel/src/task.rs`

## Implementation Steps

1. Remove local LSR/IO addresses and the private poll/write macro.
2. Change the hexadecimal helper and label/newline writes to call `probe_put`.
3. Leave the unsafe block only for the existing volatile TrapFrame read.

## Success Criteria

- [x] No BCM mini-UART address or TX-ready literal remains in the scoped task probe.
- [x] The emitted byte sequence remains structurally identical.

## Security Considerations

N/A.

## Risk Notes

Changing byte order would reduce bring-up diagnostics. Revert the one-file substitution to undo the phase.

## Deviation Log

None.
