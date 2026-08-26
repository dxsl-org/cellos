---
title: "Extend BCM MMIO Profile"
status: completed
tier: medium
created: 2026-08-18
---

# Phase 01 — Extend BCM MMIO Profile

## Requirements

- Represent peripheral and local-controller spans with checked end calculation.
- Represent exact GPIO and AUX grant-window sizes.
- Keep the crate `no_std` and data-only.
- Test exact BCM2837 values, nonzero sizes, overflow safety, and containment.

## Related Code Files

- `hal/soc/bcm27xx/src/profile.rs`
- `hal/soc/bcm27xx/src/tests.rs`

## Todo List

- [x] Add minimal span/window fields.
- [x] Add checked accessors or validation.
- [x] Extend profile tests.

## Risk Assessment

Wrong lengths can over-map MMIO or broaden grants. Rollback removes the new fields and restores existing constants; no persistent format changes.

## Success Criteria

BCM tests prove exact parity with current constants and reject overflow-prone profile data.
