---
phase: 8
title: "Record Software Completion and Parked Gates"
status: in-progress
priority: P1
effort: "0.5d"
dependencies: [1]
tier: fast
---

# Phase 08: Record Software Completion and Parked Gates

> **Required — deviation-log:** Record every decision, deviation, or surprise when it occurs. Escalate irreversible or public-contract changes.
## Context Links

- `docs/project-roadmap.md`
- `docs/project-changelog.md`
- `docs/app-tier-acceptance-ledger.json`
- `.agents/TODO.md`


## Overview

Project each lane's status as soon as that lane reaches a new evidence ceiling; never wait for unrelated lanes.
## Key Insights

Completion is per capability and evidence environment; this projection is repeatable, and aggregate product qualification may remain `NOT_COMPLETE`.


## Requirements

- Update living roadmap, changelog, architecture, risk register, child-plan status, and authoritative acceptance ledger only for the triggering lane.
- Preserve `NOT_COMPLETE` for any aggregate that still requires physical/service/security/human evidence.
- Record canonical `execution_class` and `evidence_ceiling` independently; never encode both meanings in one status.
- Remove stale TODOs only when their owning code/evidence is complete.

## Architecture

Each workstream emits an immutable record with `capability`, `environment`, `revision`, `result`, canonical `execution_class`, canonical `evidence_ceiling`, and `remaining_gate`. Phase 08 is the sole writer of roll-up views.

## Assumptions

None — status derives only from completed child evidence.

## Related Files

- Modify: `docs/project-roadmap.md`
- Modify: `docs/project-changelog.md`
- Modify: `docs/system-architecture.md` when public contracts changed
- Modify: `docs/roadmap/current-focus.md`
- Modify: `docs/roadmap/open-risk-register.md`
- Modify: `docs/app-tier-acceptance-ledger.json` only through its validator contract
- Modify: owning `.agents/*/plan.md` and `.agents/TODO.md`

## Implementation Steps

1. Accept one lane-emitted evidence/status record; never read partially written lane status from shared ledgers.
2. Validate its claim against the exercised environment and owning acceptance contract.
3. Mark that lane's software ceiling without altering unrelated physical/service/governance blockers.
4. Publish the exact asset, account, approval, or physical action that resumes that lane.
5. Run the affected documentation cross-reference, ledger, and plan-structure validators.

## Todo List

- [ ] Collect exact evidence and ceiling from the triggering lane.
- [ ] Update authoritative ledgers and living docs.
- [ ] Publish the reopening event when the triggering lane remains parked.

## Success Criteria

- [ ] No active roadmap item is globally blocked merely because another product stage lacks hardware.
- [ ] Every completed software lane points to reproducible evidence and an explicit ceiling.
- [ ] Every external lane remains visible with a concrete reopening event.
- [ ] G3 remains procurement/license/probe-only; production identity remains ADR-0006 blocked.

## Security Considerations

Status wording is a security boundary. `verified`, `qualified`, `production`, `hardware-backed`, and `admissible` require their exact evidence class.

## Risk Assessment

Do not copy evidence summaries manually when a maintained ledger or generated projection exists; drift would recreate the current blockage ambiguity.

## Next Steps

Repeat this projection after every lane transition; produce an optional final roll-up only when useful, never as a completion gate.

## Deviation Log

- Projected the completed Phase 01 documentation lane into the roadmap entrypoint, topic pages, task ledger, and changelog. The lane is `execution_class=ready` and `evidence_ceiling=contract`; no code, acceptance-ledger, physical, service, or production status changed.
- Projected Phase 03's attempted QEMU evidence run as `execution_class=governance-gated`, `evidence_ceiling=qemu`. `run.ps1` regenerated the disk rather than accepting a missing image, then `gen_disk.ps1` correctly refused F1 signing; this does not alter any admission, hardware, service, or production status.
- Recorded the user-selected relay-first Cell-to-Cell contract as `execution_class=scope-gated`, `evidence_ceiling=host`. The recovery-plan Phase 01 needs an approved test-only K1 image fixture before local IPC, queue/cache, and saturation baselines can be measured; current broker code has no constructed direct Noise or relay-routing path, so no runtime evidence changed.
- Reclassified the RPi3 HDMI lane as `execution_class=governance-gated`, `evidence_ceiling=host` after the F1 scanner rejected the unapproved BCM mailbox unsafe copies. Existing implementation is not a passing software result; no physical framebuffer, coherency, visual, or production status changed.
- Attempted to project Phase 06 QEMU runner output. The projection was retracted because its parser treated guest-authored `NOT_APPLICABLE` classifications and self-authored reset/budget markers as evidence.
- Correction: Phase 06 is not complete. The x86 runner retains a real CPU-bound budget stimulus with outer-QEMU liveness, but its guest reset stimulus produces neither nested-VMM exit nor supervisor restart; bounds/descriptor/backend cannot reach VMM/VirtIO and VMM preemption remains unobserved. The lane remains `scope-gated`; ARM64 remains `NOT_APPLICABLE`.
- Phase 07 now has an approved GitHub Actions Native Attestations policy limited to software/QEMU evidence. The CI workflow stages and attests a revision/run-sequence-bound bundle, but no workflow run has been independently verified, so the lane is `scope-gated` and no status projection is emitted.
- Superseded the earlier HDMI governance-gated projection after `lungmat8` approved the exact unsafe island and the software plus RPi3-B physical development gates passed. The lane is completed/regression-only at the `physical` development evidence ceiling; the TFTP transfer record, later UART boot block, and user visual observation remain separate evidence sources and do not imply production qualification.
