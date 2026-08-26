---
phase: 3
title: "Move Kernel LOC To Generated Status"
status: complete
priority: P1
effort: "2h"
dependencies: []
tier: fast
---

# Phase 3: Move Kernel LOC To Generated Status

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Overview

Close D3 by choosing one definition and one owner: generated project status owns moving LOC numbers; specs cite that status and define scope, not stale totals.

## Requirements

- Functional: Remove or qualify frozen kernel LOC values from normative docs.
- Non-functional: Keep the kernel-boundary target as a responsibility target, not a false measured claim.

## Architecture

Observed contradictions:
- `docs/specs/00-context.md:195`, `docs/specs/12-reliability.md:91`, `docs/specs/12-reliability.md:306`, and `docs/specs/16-rustc-tcb.md:142` still cite `~11.5K`.
- `docs/specs/15-kernel-boundary.md:323` still lists `<= 5,000` in the comparison table.
- `docs/system-architecture.md:47` and `docs/system-architecture.md:928` cite an older `~22.6K` measurement.
- `docs/project-overview-pdr.md:57` already has the right direction: kernel size is tracked by generated project status.

Definition: report `kernel/src` nLOC excluding tests as the main number, with a separate "core nLOC excluding tests, drivers, and hypervisor" lens for the Spec 15 driver-migration target.

## Assumptions

- **Claim:** A generated status file either exists or can be added under the existing docs/status pattern.
  **Confidence:** medium
  **How to verify:** inspect Spec 21/status tooling before writing any generator reference.

## Related Files

- Modify: `docs/specs/00-context.md`
- Modify: `docs/specs/12-reliability.md`
- Modify: `docs/specs/15-kernel-boundary.md`
- Modify: `docs/specs/16-rustc-tcb.md`
- Modify: `docs/system-architecture.md`
- Modify: `docs/project-overview-pdr.md`
- Maybe create/modify: generated status artifact specified by Spec 21

## Implementation Steps

1. Find the Spec 21 generated-status owner and status file path.
2. Define the measurement command in that owner, not in every spec.
3. Replace prose totals with "see generated status" plus the chosen definition.
4. Restate Spec 15 G2 `<=5,000 core` as a target measured against core nLOC, or withdraw it if not currently binding.
5. Record D3 as ruled in the docket/report.

## Success Criteria

- [x] Exactly one document/status artifact owns the moving kernel LOC measurement.
- [x] Normative specs no longer freeze raw current totals.
- [x] Spec 15 states whether `<=5,000 core nLOC` remains binding.
- [x] `git diff --check` passes.

## Security Considerations

Kernel LOC is a TCB communication tool. Stale understatement can mislead safety/security review, so evidence provenance must be explicit.

## Risk Notes

- Likelihood high, impact medium: replacing numbers with generated-status pointers can feel less concrete. Mitigation: keep definitions and commands visible in the owner.
- Rollback: restore previous prose values. Irreversible part: none.

## Deviation Log

None.
