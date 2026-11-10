---
phase: 0
title: "Governance, admission and owner decisions"
status: completed
priority: P1
effort: "1–2d"
dependencies: []
tier: thinking
checkpoint: false
---

# Phase 00: Governance, admission and owner decisions

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Context Links

[Plan](plan.md) · [Review](../reports/architecture-review-261110.md) · [Lock register](../cell-contract-0024-locks.md) · [Portfolio](../plan-portfolio.md) · [ADR-0022](../../docs/decisions/0022-intel-x86-64-c2c-only-direction.md) · [Red team](reports/red-team.md) #1, #6, #10

## Overview

Obtain the owner's admission of this plan, the four design decisions and the slice order. No product file changes; it exists as its own phase because nothing else may start without it.

## Key Insights

- Every later slice edits locked surfaces; the repository's own rule is a file-bounded exception per slice, ended at slice close.
- The review is a finding list, not a mandate. ADR-0022 requires each task to name the Intel C2C deliverable it serves; remediation serves it as "measured defect" work.

## Requirements

- Functional: an approval record in this directory answering YES/NO per item; a portfolio row; a lock-register entry for slice S0 only.
- Non-functional: no approval is inferred from silence; later slices get their own requests.

## Architecture

Admission artifacts, following the existing convention (`s2-handoff-request.md` / `s2-handoff-approval.md` in the x86 breadth plan):

- `admission-request.md`: scope, slice list, per-slice file-set outline, Law-1 deltas expected (phases 06, 08, 11, 12, 13), evidence ceiling, the four decisions with defaults, conflicts with active slices.
- `admission-approval.md`: the owner's answers, dated.

## Assumptions

- **Claim:** no other slice currently holds the Intel WIP lane.
  **Confidence:** low
  **How to verify:** read the tail of `.agents/cell-contract-0024-locks.md` and `.agents/plan-portfolio.md` for an open "holds the Intel WIP lane" entry.
- **Claim:** remediation qualifies under ADR-0022 as dependency/defect work and needs no ADR amendment.
  **Confidence:** medium
  **How to verify:** owner answer in `admission-approval.md`; if NO, draft an ADR amendment before Phase 01.

## Related Code Files

- Create: `.agents/261110-1703-architecture-remediation/admission-request.md`, `admission-approval.md`
- Modify: `.agents/plan-portfolio.md` (one row), `.agents/cell-contract-0024-locks.md` (S0 exception entry)

## Implementation Steps

1. Read the lock register and portfolio; list every active or approved-not-effective slice and its file set.
2. Write `admission-request.md` with: slices S0–S8, the minimum cut (S0–S2), expected Law-1 deltas, and the four decisions from `plan.md` with defaults and consequences.
3. Mark file-set collisions with active slices and propose ordering.
4. Ask the owner for explicit answers (ask tool); record them verbatim in `admission-approval.md`.
5. Add the portfolio row and the S0 lock exception (paths of Phase 01 only).

## Todo List

- [x] Active-slice inventory
- [x] Admission request written
- [x] Owner answers recorded
- [x] Portfolio row and S0 exception added

## Success Criteria

- [x] `admission-approval.md` contains an explicit answer for plan admission, slice order and each of the four decisions.
- [x] Lock register shows a file-bounded exception for S0 and no wider grant.

## Risk Assessment

- Owner declines or narrows scope: record it, re-cut `plan.md` to the admitted slices, stop. Rollback: delete the two records and the portfolio row; nothing irreversible.
- Collision with an active slice: this plan waits; it never takes files from an Intel slice.

## Security Considerations

N/A — records only.

## Next Steps

Phase 01, limited to the admitted S0 file set.

## Deviation Log

- Decision: recorded x86 breadth S4 as closed and the Intel WIP lane free on the owner's form answer (`lane = record-s4-closed`).
  Why: S4 held the lane since 2026-11-07 with no close record; S1 of this plan would otherwise collide.
  Impact: `.agents/cell-contract-0024-locks.md` S4 entry (one sub-bullet).
  Revert: delete that sub-bullet.
- Surprise: x86 breadth S5 NIC driver code landed (commits `9297e3404`…`aff9ff81c`) with no handoff record.
  Why: found during the active-slice inventory.
  Impact: none here; noted in `admission-approval.md` for the x86 breadth owner.
  Revert: n/a.

## Evidence

[admission-request.md](admission-request.md), [admission-approval.md](admission-approval.md); lock register S0 entry and S4 close note; portfolio section "Admitted — architecture remediation".
