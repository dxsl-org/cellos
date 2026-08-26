---
title: "BCM27xx IRQ Topology"
description: "Move immutable BCM2837 IRQ numbers and local-source masks into SoC data."
status: completed
priority: P2
effort: 3h
branch: fix/structure
tags: [refactor, hal, bcm27xx, irq]
created: 2026-08-18
---

# BCM27xx IRQ Topology

## Scope Contract

- Add immutable BCM2837 legacy IRQ numbers and BCM2836 Core0 source masks to `hal/soc/bcm27xx`.
- Preserve existing public ARM HAL constants as aliases to SoC facts.
- Make the BCM2835 system-timer interrupt enable/pending path consume its SoC IRQ number.
- Keep register offsets, enable/disable/ack/dispatch mechanisms, timer period, kernel paths, and board data unchanged.

## Phases

| Phase | Name | Status | Depends |
|---|---|---|---|
| 1 | [Add and validate IRQ topology](./phase-01-add-irq-topology.md) | completed | none |
| 2 | [Consume topology in ARM HAL](./phase-02-consume-irq-topology.md) | completed | 1 |
| 3 | [Verify, review, and document](./phase-03-verify-review-document.md) | completed | 2 |

## Compatibility Strategy

Public constant names and every computed mask remain numerically identical. Only immutable-value ownership changes.

## Deferred Work

- Timer-frequency and scheduler-quantum policy.
- Multi-core routing policy and executable board pinmux generation.
- Physical RPi3 validation and board-feature collapse.

## Evidence Boundary

QEMU RV64 runtime passed as a regression gate. RPi3 remains compile-only for this slice.
