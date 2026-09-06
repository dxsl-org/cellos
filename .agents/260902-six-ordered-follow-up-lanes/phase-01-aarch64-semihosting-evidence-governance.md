---
phase: 1
title: "AArch64 Semihosting Evidence and Governance"
status: completed
dependencies: []
tier: thinking
---

# Phase 01: AArch64 Semihosting Evidence and Governance

## Context Links

- [Master plan](plan.md) · [Research](research/aarch64-ledger.md) · [Scout](scout-report.md)
- [ADR-0013](../../docs/decisions/0013-solo-first-development-independent-promotion.md) supersedes actor separation and cross-lane serialization in this plan.
- `scripts/qemu-aarch64-test-hooks.sh:43-72,88-124`
- `docs/app-tier-acceptance-ledger.json:6-15,70-88`
- `scripts/app_tier_acceptance/ledger.py:159-211`
- `scripts/app_tier_acceptance/events.py:21-105`

## Overview

Capture immutable runtime evidence first. The accountable maintainer may also
design and implement the schema-v4 append-only governance mechanism. Ledger
promotion occurs only after a distinct repository member approves the exact
claim, proposal, commit/tree, and evidence through Issue #47; this phase never
advances acceptance-ledger production Phase 3.

## Key Insights

- Current baseline logic makes blocker `id/subject/scope/evidence` immutable, so an ordinary lifecycle event cannot correct the contradiction.
- The existing runner is the oracle; marker excerpts, old root logs, or a plan status are not evidence.
- One accountable maintainer may perform Evidence Runner, Ledger Steward,
  implementation, test, documentation, and merge duties. Only the promotion
  decision requires a distinct repository member.

## Requirements

- Preserve exactly two unique non-ignored `.txt` artifacts: untouched raw QEMU output and complete build/runner transcript containing the final PASS.
- Bind command, UTC time, revision/tree, ELF SHA-256, QEMU path/version/binary SHA-256, exit status, artifact byte counts/SHA-256, owner, runner, and live resolution TTL.
- Ratify schema v4 before changing production ledger bytes. Support old event replay and new non-lifecycle `schema_migration`, `record_correction`, and `blocker_resolution` actions.
- Correction changes only subject inventory plus blocker subject/scope; it keeps blocker ID, historical evidence, status `BLOCKED`, and resolution null. A later trusted-parent resolution changes only status/resolution to `PASS`.
- Keep ledger lifecycle Phase 3 exactly `PLANNED`. Preserve test-hook-only production defaults and every non-claim.

## Architecture

`fresh run -> two hashed artifacts -> tested v4 mechanism -> bound GitHub decision -> v3→v4 migration event -> correction event -> resolution event`. Every event appends one hash-chain node, names its trusted parent, records exact before/after section digests and rationale, and leaves lifecycle unchanged. Validator rejects action/data deltas not permitted for that action.

Commit boundaries are strict: **01A** evidence only; **01B** validator/schema/tests and the exact proposal, no ledger data; after a bound `DECISION: YES`, **01C** one migration event; **01D** one correction event leaving BLOCKED; **01E** one later resolution event after freshness and decision-binding recheck; **01F** current verification/changelog wording bound to the exact tested 01E revision/tree, commands, and artifact hashes. Each candidate validates against its immediate trusted parent.

## Related Code Files

- Modify: `scripts/app_tier_acceptance/events.py`, `ledger.py`, `validator.py`, `scripts/validate-app-tier-acceptance.py`
- Modify: `tests/app-tier-acceptance/test_acceptance_ledger.py`, `test_review_regressions.py`
- Modify only after ratification: `docs/app-tier-acceptance-ledger.json`
- Create at execution: `docs/evidence/aarch64-semihosting-<event>-raw.txt`, `...-runner.txt`
- Triggered after accepted resolution: `docs/roadmap/open-risk-register.md`, `docs/project-roadmap.md`, current `[Unreleased]` changelog
- Retain: seed fixture/history, matrix-derived values, production runners, ABI, kernel outside a reproduced test-hook defect

## Execution and Evidence Note (2026-09-02)

- Baseline validation was green. The diagnostic ARM execution then completed with build status `0`, runner status `0`, and outer pipeline status `0`.
- The retained raw artifact is `docs/evidence/aarch64-semihosting-20260902-01-raw.txt`, SHA-256 `4e95514712074e077fa88c871c699aa7d8fcc039b26aa3f830f266e4b2275925`, 29,571 bytes.
- The retained runner transcript is `docs/evidence/aarch64-semihosting-20260902-01-runner.txt`, SHA-256 `6527744a11e110ec550ed15a83e970280f58b57fa3c187d1e4be44fa75e4016b`, 17,032 bytes.
- These artifacts record diagnostic runtime success only. Subagents within one
  operator session provide automated assurance but no independent promotion;
  no distinct repository member approved the exact bound inputs.
- No ledger or schema changes were made, and the ledger SHA remained unchanged.
  Issue #47 is the promotion channel. Its proposal, commit/tree, and evidence
  fields must become exact and immutable before a decision is requested.
- Only Phase 01 ledger promotion is halted. The diagnostic artifacts are
  non-qualifying and every Phase 01 promotion criterion remains unchecked;
  unrelated lanes may proceed under ADR-0013.

## Implementation Steps

1. Name the accountable maintainer and an event ID; ensure `docs/evidence/` names do not exist and remove only scratch root runner logs. The maintainer may hold all execution and stewardship roles.
2. Remove only the scratch raw log, record start time, then under `set -o pipefail` tee one subshell transcript that initializes `BUILD_RC=125 RUNNER_RC=125`, records provenance, executes `{ build; BUILD_RC=$?; test "$BUILD_RC" -eq 0; } && { runner; RUNNER_RC=$?; test "$RUNNER_RC" -eq 0; }`, prints both statuses, and exits with the guarded chain status. Retain outer `${PIPESTATUS[0]}`; never run the runner after build failure.
3. Require outer/build/runner status zero plus a newly created raw stream newer than start; copy that untouched raw stream to `...-raw.txt`, then record SHA-256 and byte count for raw and transcript. On any nonzero, timeout, fault, missing marker/final PASS, or stale/copied output, stop Phase 01 promotion without touching ledger/fixture/matrix. Other lanes remain executable.
4. Implement backward replay plus v3→v4 migration validation and commit it as
   01B with no ledger-data change. Add adversarial tests for replay, extra keys,
   unbound digest, altered history, lifecycle drift, bundled
   correction/resolution, wrong subject/architecture, missing or mismatched
   GitHub decision binding, stale TTL, absent raw log, and nonzero
   build/runner/transcript.
5. Bind the exact claim, 01B commit/tree, and evidence URLs in Issue #47, then
   request `DECISION: YES` or `DECISION: NO` from one distinct repository
   member. Without `YES`, retain diagnostic evidence and stop before ledger
   events; no unrelated lane is blocked. Any material change to 01B or the
   evidence invalidates the decision.
6. After the bound `YES`, append and validate migration, correction, and later resolution one commit/event at a time. The correction introduces `qemu-arm64` and factual semihosting execution/termination scope; resolution binds only the approved fresh ARM evidence.
7. For each candidate run `python3 scripts/validate-app-tier-acceptance.py --baseline <immediate-parent-ledger.json> --baseline-root <immediate-parent-checkout>` and `python3 -m unittest discover -s tests/app-tier-acceptance -p 'test_*.py'`.
8. Only after 01E passes from the tested commit/tree, append current risk/roadmap/changelog verification with that revision/tree, literal commands/results, separate build/runner/outer statuses, evidence paths/SHA-256/sizes, QEMU test-hook-only scope, and production/physical/C9 exclusions.

## Todo List

- [ ] Capture and hash fresh raw/transcript artifacts.
- [ ] Implement and adversarially validate non-lifecycle append-only actions.
- [ ] Bind the exact claim, proposal commit/tree, and evidence in Issue #47.
- [ ] Obtain a distinct repository member's `DECISION: YES` before promotion.
- [ ] Append migration, correction, and resolution against successive trusted parents.
- [ ] Bind final verification/changelog to the exact tested source commit/tree and artifacts.
- [ ] Confirm ledger Phase 3 remained `PLANNED` and documentation claims stayed bounded.

## Success Criteria

- [ ] Build, runner, and outer pipeline each exit 0; build failure never executes a stale runner, transcript contains both statuses and final PASS, and raw stream is untouched, unique, hashed, sized, and fresh.
- [ ] Every v4 event validates against its immediate parent; old history bytes/hash chain replay unchanged and each before/after digest matches the exact allowed delta.
- [ ] Corrected blocker names an AArch64 QEMU subject; PASS resolution binds the two fresh artifacts and the exact GitHub `YES`.
- [ ] No matrix denominator, unrelated blocker, lifecycle state, production runner, or acceptance-ledger Phase 3 state changes.
- [ ] Missing evidence, bound `YES`, or test success leaves only Phase 01 promotion pending; no ledger correction or promotion occurs.
- [ ] Normal verification/changelog records the exact tested 01E commit/tree, literal commands/results, and evidence hashes/sizes.

## Risk Assessment

- The runner may expose a real termination defect; repair only that reproduced test-hook defect, rerun from clean evidence names, and keep governance blocked meanwhile.
- A schema migration can accidentally legalize arbitrary history edits. Restrict action-specific deltas, replay old events, and reject combined correction/resolution.
- TTL may expire during review. Rerun under the same accountable maintainer
  rather than extending timestamps or copying evidence; bind the replacement
  artifacts to a new GitHub decision.
- Rollback is the last trusted parent; never delete/rewrite events or append a compensating fiction.

## Security Considerations

Evidence files must be regular repository-relative non-symlinks with exact hashes/sizes. Role labels and distinct strings do not create human independence; only the bound GitHub decision by another member does. Never force-add ignored scratch logs, relax freshness/decision/schema checks, or let semihosting alter production boot defaults.

## Assumptions

- **Claim:** A distinct repository member will review the immutable bound inputs. **Confidence:** medium. **How to verify:** require exact `DECISION: YES` or `DECISION: NO` in Issue #47; do not request it while any input is pending.
- **Claim:** A fresh QEMU run can complete before the chosen live TTL expires. **Confidence:** medium. **How to verify:** execute the owned runner and inspect timestamps/status.
- **Claim:** Governance will accept v4 non-lifecycle events. **Confidence:** low. **How to verify:** obtain the bound distinct-member `YES`; otherwise stop only Phase 01 promotion.

## Next Steps

Phase 01 proceeds through development and evidence collection under the sole
maintainer. Ledger events wait for the bound distinct-member `YES`; a missing or
negative decision blocks only this promotion. Never confuse this plan's Phase 01
with acceptance-ledger lifecycle Phase 1 or 3.
