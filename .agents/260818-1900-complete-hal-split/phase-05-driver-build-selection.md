---
phase: 5
title: "Drive Build Selection From Typed Board Data"
status: completed
priority: P2
effort: "5h"
dependencies: [2, 3, 4]
tier: medium
---

# Phase 5: Drive Build Selection From Typed Board Data

## Overview

Make typed enabled-driver data enforceable and add one reproducible matrix for every current board configuration.

## Requirements

- Provide allocation-free `has_driver` checks and use them at shared-driver initialization boundaries.
- Keep Cargo feature names as compatibility selectors, not mechanism forks.
- Add a bounded script that checks all board configurations and descriptor assets.
- Detect incompatible simultaneous board selections at compile time.

## Architecture

Cargo chooses one board package; typed board data determines optional shared-driver initialization.

## Assumptions

None — active driver initialization call sites will be inventoried before edits.

## Related Files

- Modify: `boards`, kernel board/driver init, Cargo feature guards
- Create: `scripts/check-board-configs.sh`
- Modify: board READMEs

## Implementation Steps

1. Add typed driver queries and selection guards.
2. Replace remaining board-feature driver-init decisions with descriptor capability checks where runtime-safe.
3. Add and run the all-board compile matrix.

## Success Criteria

- [x] Every board README command is exercised by the matrix.
- [x] No board selects a driver by unvalidated string comparison.

## Security Considerations

Invalid or conflicting selections fail at build/startup rather than probing guessed hardware.

## Risk Notes

Over-eager runtime conversion can initialize unavailable devices. Preserve early compile gating where symbol availability requires it; document any justified cfg.

## Deviation Log

None.
