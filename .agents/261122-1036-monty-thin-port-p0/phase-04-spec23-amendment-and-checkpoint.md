---
phase: 4
title: "Spec 23 amendment and slice-A checkpoint"
status: pending
priority: P1
effort: "1d"
dependencies: [3]
tier: medium
checkpoint: true
---

# Phase 4: Spec 23 amendment and slice-A checkpoint

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Overview

This phase re-binds the ratified Spec 23 text to the new pin through the ledger's prose-only `source_amendment`
event. It then runs the full slice-A checkpoint on all three architectures, including the boot, Tier-2 domain and
hostile lanes.

## Requirements

- Functional: Spec 23 §3.1 (`docs/specs/23-native-sdk-contract.md:107-119`) names `N`. Nothing else in the spec changes.
  The matrix digest covers only §5 (`scripts/app_tier_acceptance/source.py:44-58`), so the `source_amendment` rules hold
  (`ledger.py:260-274`).
- Functional: exactly one new ledger event in the push (trusted-baseline rule; `docs/project-changelog.md` 2026-11-20 entry).
- Functional: the spec edit (A), its archived snapshot and the ledger event (B) land in **one push**, after reviewer approval.
  A alone turns CI red: `app_tier_acceptance/validator.py:41` compares the live spec digest with `source_binding`.
- Functional: new perf baselines are recorded for `N`. History resets because the compatibility key includes the toolchain.

## Architecture

```mermaid
sequenceDiagram
  participant S as Steward
  participant R as Reviewer
  S->>S: commit A: Spec 23 §3.1 prose (pin N)
  S->>S: archive docs/evidence/spec23-native-sdk-contract-<sha12>.md (bytes of A)
  S->>S: commit B: source_amendment event (ratified_revision = A), event 14 as template
  S->>R: request review of A+B
  R-->>S: approve
  S->>S: push A+B together; CI validates against trusted baseline
```

## Assumptions

- **Claim:** event 14 is a valid template, and `--baseline`/`--baseline-root` validate a candidate locally.
  **Confidence:** high (verified by red-team). **How to verify:** `jq '.events[13]' docs/app-tier-acceptance-ledger.json`.
- **Claim:** the steward and reviewer roles are available.
  **Confidence:** unknown. **How to verify:** ask the user for names; never self-approve.

## Related Files

- Modify: `docs/specs/23-native-sdk-contract.md` (§3.1 only)
- Create: `docs/evidence/spec23-native-sdk-contract-<sha12>.md`
- Modify: `docs/app-tier-acceptance-ledger.json`, `docs/app-tier-acceptance-matrix.md`
- Modify: `docs/roadmap/runtime-and-platform-tracks.md:107-134`, `docs/project-changelog.md`, `CHANGELOG.md`
- Modify: perf baseline files as written by the perf workflow tooling

## Implementation Steps

1. Edit §3.1 (lead sentence + three tuple rows). Commit A.
2. Archive A's bytes under the `source.snapshot_path` naming rule.
3. Build the event from event 14. Compute digests with the `scripts/app_tier_acceptance` helpers. Commit B.
4. `python3 scripts/validate-app-tier-acceptance.py --baseline <prior ledger> --baseline-root <checkout at prior commit>`.
5. Update the roadmap track text and the changelogs. Record new perf baselines with the perf tooling.
6. Run the checkpoint below; get reviewer approval; push A+B.

## Success Criteria

- [ ] `validate-app-tier-acceptance.py` passes on the working tree and against the trusted baseline.
- [ ] Checkpoint: `ci-local.py run` passes on `N` for `lint`, `build`, `aarch64-lint`, `x86_64-lint`, `unit-tests`, `qemu-boot`,
      `qemu-aarch64-boot`, `qemu-x86_64-boot`, `rust-std-lane`, `x86-c2c` (includes the x86 Tier-2 domain test, `ci.yml:1504-1519`),
      `rv64-tier2-disk`, `qemu-x86-tier3-hostile`, `f1-admission`, `boot-suite` and `security`.
- [ ] If `main` was already red before the change, the failing jobs are recorded first and the comparison is against that baseline.

## Security Considerations

The hostile and domain lanes are included because a compiler bump can silently change hardening codegen.
A regression there blocks slice A regardless of other results.

## Risk Notes

- Some heavy lanes (KVM, hardware) may not run locally. Record which jobs ran where. A job that did not run is
  reported as not run, never as passed.

## Deviation Log

None.
