---
title: "Phase 09 - Rollout Docs and Candidate A Contingency"
status: pending
priority: P1
effort: 2
depends_on: [08]
owner: "release-readiness"
---

# Phase 09 - Rollout Docs and Candidate A Contingency

## Context Links

- Success gates: `reports/success-gates.md`
- Red Team Review: `reports/red-team-review.md`
- Validation: `reports/validation.md`

## Overview

Priority P1. Prepare handoff criteria, living-doc updates, rollback notes, and the Candidate A Law-1 contingency gate.

## Key Insights

- Candidate B should be exhausted before touching `libs/api`.
- Documentation updates happen only after real implementation/evidence, not in this planning turn.
- COMPLETE means oracle evidence, not module presence.

## Requirements

- Functional: pre-implementation security checklist, release checklist, docs-update list, status language, contingency decision package.
- Non-functional: no Law-1 edit without two explicit confirmations; no hardware evidence claim without hardware run.

## Architecture

Data flow: pre-implementation security checklist -> oracle evidence -> red team review -> validation review -> status decision -> docs handoff -> release or keep partial status.

## Related Code Files

- Future docs owner: `docs/project-roadmap.md`
- Future docs owner: `docs/project-changelog.md`
- Future docs owner: `docs/system-architecture.md`
- Candidate A possible Law-1 owner: `libs/api/src/abi/syscall.rs`, `libs/ostd/src/syscall.rs`, `kernel/src/task/syscall.rs`

## Implementation Steps

1. Carry Red Team Review findings into the implementation checklist.
2. Carry validation gates into the release checklist.
3. Define exact status labels: partial, oracle-passed, hardware-qualified.
4. Define Candidate A entry checklist.
5. Define rollback and operations notes.
6. Define pre-implementation security checklist for relay auth, key lifecycle, exports, replay, and log redaction.

## Todo List

- [x] Root filled Red Team Review result.
- [x] Root filled validation result.
- [ ] User approves any docs status change.
- [ ] User approves implementation start from Candidate B.
- [ ] User gives two Law-1 confirmations only if Candidate A becomes necessary.

## Success Criteria

- No implementation claims COMPLETE without relay and LAN oracle evidence.
- Candidate A is not started unless every gate is checked.
- Rollback steps are documented for each phase.

## Risk Assessment

- Risk: pressure to mark flagship complete early. Likelihood high, impact high. Mitigation: success gates require two-node oracle.
- Risk: Law-1 contingency becomes default. Likelihood medium, impact high. Mitigation: Candidate A entry conditions are explicit and hard.

## Security Considerations

Docs must keep remote trust boundary visible: node-level auth, explicit exports, no remote VFS/watch ABI in V1.

## Rollback

Rollback is status-only until implementation exists. If Candidate A is later attempted, rollback requires reverting ABI additions and all users, so it needs its own Law-1 package.

## Next Steps

Root has finalized Red Team Review and validation; implementation still waits for user approval of Candidate B.
