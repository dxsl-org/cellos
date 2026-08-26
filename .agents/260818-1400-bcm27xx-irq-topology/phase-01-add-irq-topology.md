---
title: "Add IRQ Topology"
status: completed
tier: medium
created: 2026-08-18
---

# Phase 01 — Add IRQ Topology

## Requirements

- Represent system-timer C1, AUX, and GPIO-bank legacy IRQs.
- Represent Core0 timer NS/HP and GPU source masks.
- Validate ranges, uniqueness, and exact current values.

## Todo List

- [x] Add the topology type and BCM2837 values.
- [x] Add unit tests for exact values and invariants.

## Risk Assessment

Wrong topology can silently drop or misroute interrupts. Rollback removes the topology and restores ARM-local constants.

## Success Criteria

Host tests prove exact parity and non-overlapping source masks.
