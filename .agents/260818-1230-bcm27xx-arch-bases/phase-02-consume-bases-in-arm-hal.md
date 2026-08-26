---
title: "Consume Bases In ARM HAL"
status: completed
tier: medium
created: 2026-08-18
---

# Phase 02 — Consume Bases In ARM HAL

## Requirements

- Add target-scoped optional `hal-soc-bcm27xx` dependency to `hal-arm`.
- Activate it only through `board-rpi3`.
- Replace base literals in five BCM-specific ARM modules without moving offsets or mechanisms.

## Todo List

- [x] Wire the optional dependency and feature.
- [x] Rewire UART, IRQ, timer, and diagnostic bases.
- [x] Confirm no targeted absolute base literal remains.

## Risk Assessment

Incorrect feature propagation could break non-RPi3 AArch64 or other architectures. Rollback restores the empty feature and local constants.

## Success Criteria

AArch64 default and RPi3 builds pass with identical computed register addresses and no cross-target dependency leakage.
