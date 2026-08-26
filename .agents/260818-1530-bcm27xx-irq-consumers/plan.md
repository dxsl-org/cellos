---
title: "BCM27xx IRQ Consumer Closure"
description: "Route remaining BCM IRQ consumers through the centralized SoC topology."
status: completed
priority: P2
effort: 2h
branch: fix/structure
tags: [refactor, hal, bcm27xx, irq]
blockedBy: []
blocks: []
created: 2026-08-18
---

# BCM27xx IRQ Consumer Closure

## Scope Contract

- Derive BCM2835 legacy pending-bank masks from the public IRQ aliases.
- Route the RPi3 CNTP enable path through the exported local-source mask.
- Make the kernel RPi3 IRQ diagnostic consume BCM2837 MMIO and IRQ facts.
- Preserve register offsets, C1 status/ack semantics, timer policy, diagnostic bytes, and public constants.

## Phases

| Phase | Name | Status | Depends |
|---|---|---|---|
| 1 | [Close ARM HAL consumers](./phase-01-close-arm-hal-consumers.md) | completed | none |
| 2 | [Close kernel diagnostic consumers](./phase-02-close-kernel-diagnostics.md) | completed | 1 |
| 3 | [Verify, review, and document](./phase-03-verify-review-document.md) | completed | 2 |

## Deferred Work

Timer frequency/policy, scheduler quantum, multi-core routing, UART debug wiring in `task.rs`, pinmux, and physical RPi3 validation remain separate slices.

## Evidence Boundary

QEMU RV64 passed as a regression lane. RPi3 remains compile-only.
