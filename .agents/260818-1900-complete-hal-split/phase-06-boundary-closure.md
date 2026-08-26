---
phase: 6
title: "Enforce Boundaries And Close Documentation"
status: completed
priority: P1
effort: "4h"
dependencies: [5]
tier: medium
---

# Phase 6: Enforce Boundaries And Close Documentation

## Overview

Add regression guards, run the full matrix, and remove the documented deferred status.

## Requirements

- Guard against board MMIO/IRQ facts returning to `hal/arch` or generic driver mechanisms.
- Guard against per-board copies of shared driver families.
- Run host tests, every board compile lane, release RV64, and QEMU boot.
- Update living docs with exact completed/deferred evidence.

## Architecture

Boundary tests enforce ownership while allowing register offsets and guest-emulation contracts in their legitimate layers.

## Assumptions

None.

## Related Files

- Create/modify boundary-check scripts and tests
- Modify: `docs/project-{roadmap,changelog}.md`, `docs/system-architecture.md`, `docs/code-standards.md`

## Implementation Steps

1. Add narrow ownership/duplication guards with explicit allowlists.
2. Run the complete matrix and reviewer passes.
3. Sync plan/docs and record physical hardware gates separately.

## Success Criteria

- [x] Completion Contract is mechanically checked where possible.
- [x] All supported build configurations pass and both RV64/AArch64 QEMU witnesses boot.
- [x] No HAL-split work remains deferred except physical-hardware runtime evidence.

## Security Considerations

Do not weaken allowlists or fail-closed access policies to make guards pass.

## Risk Notes

Over-broad grep guards cause false positives. Keep them scoped and allow mechanism offsets/tests explicitly.

## Deviation Log

None.
