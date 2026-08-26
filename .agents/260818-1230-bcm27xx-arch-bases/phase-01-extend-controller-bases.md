---
title: "Extend Controller Bases"
status: completed
tier: medium
created: 2026-08-18
---

# Phase 01 — Extend Controller Bases

## Requirements

- Add system-timer and legacy-IRQ bases to the data-only profile.
- Validate exact offsets and containment in the peripheral span.
- Keep the profile `no_std` and free of register behavior.

## Todo List

- [x] Add the two base facts.
- [x] Extend BCM2837 tests.

## Risk Assessment

Wrong bases would fault early boot. Rollback removes the fields and restores existing arch literals; no persistent state changes.

## Success Criteria

Host BCM tests prove exact current addresses and containment.
