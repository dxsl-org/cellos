---
title: "RPi3 UART Debug Driver Reuse"
description: "Remove the kernel TrapFrame probe's private mini-UART write implementation."
status: completed
priority: P2
effort: 1.5h
branch: fix/structure
tags: [refactor, hal, rpi3, uart]
blockedBy: []
blocks: []
created: 2026-08-18
---

# RPi3 UART Debug Driver Reuse

## Scope Contract

- Replace the `task.rs` TrapFrame probe's raw mini-UART poll/write macro with `uart_bcm_mini::probe_put`.
- Preserve the AArch64/RPi3 cfg gate, hexadecimal formatting, labels, newlines, volatile TrapFrame read, and task setup order.
- Do not change UART init, pinmux, baud, IRQ/RX handling, SoC data, or public APIs.

## Phases

| Phase | Name | Status | Depends |
|---|---|---|---|
| 1 | [Reuse the ARM HAL byte writer](./phase-01-reuse-hal-byte-writer.md) | completed | none |
| 2 | [Verify parity and regression](./phase-02-verify-parity.md) | completed | 1 |
| 3 | [Review and document](./phase-03-review-document.md) | completed | 2 |

## Evidence Boundary

QEMU RV64 passed as the runtime regression lane. RPi3 remains compile-only.
