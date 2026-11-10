---
phase: 2
title: "x86 C2C gate, lane manifest, reproducible CI"
status: pending
priority: P1
effort: "4–6d"
dependencies: [1]
tier: medium
checkpoint: false
---

# Phase 02: x86 C2C gate, lane manifest, reproducible CI

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Context Links

[Review H-14, Medium build/repo](../reports/architecture-review-261110.md) · `.github/workflows/ci.yml` · `scripts/qemu-two-node-c2c-test.sh` · `scripts/x86/qemu-domain-test.sh` · [Spec 10](../../docs/specs/10-testing.md)

## Overview

Put the stated direction under a per-commit gate before any kernel refactor, and make it visible which lanes run where. Regression net for S2–S8.

## Key Insights

- Reviewer-cited: two-node C2C and x86 domain scripts appear in no workflow; 53 of 71 lane scripts and 34 of 50 integration targets are unreferenced; `--locked` appears zero times in `ci.yml`; no `concurrency:` block.
- The x86 image recipe (`scripts/assemble-x86-image.py` → `scripts/x86/make-iso-ci.sh`) is already single-source and is the model.

## Requirements

- Functional: one `x86-c2c` job (two-node TCG test incl. `C2C_NEGATIVE=1`, x86 domain test, `local-service-lifecycle-x86`); a checked-in lane manifest; a guard that fails on unlisted lane scripts and on host-testable crates missing from the test list; x86-64 workspace clippy.
- Non-functional: lockfile respected in CI; superseded runs cancelled; every job has a timeout.

## Architecture

- `scripts/lanes.toml`: one row per lane script or integration target — `path`, `arch`, `tier = pr | nightly | manual | retired`, `reason`.
- `scripts/check-lanes.py`: every `scripts/qemu-*`, `scripts/run-*`, `scripts/build-*-ci.sh`, `scripts/x86/qemu-*` and every `[[test]]` in `tests/integration/Cargo.toml` must be in the manifest; `pr`/`nightly` rows must be named by a workflow.
- Host-test list moves from the hand-written `-p` block (`ci.yml:1806-1885`, reviewer-cited) to `scripts/host-test-crates.txt`, checked against crates containing `#[test]`.
- Workflow level: `env: CARGO_LOCKED: "true"` (or `--locked` on each cargo call if the env form is not honoured by the pinned cargo), `concurrency` with cancel-in-progress, composite setup action for the repeated apt/pip block.

## Assumptions

- **Claim:** the two-node and domain scripts pass on a clean checkout under TCG on a hosted runner within a usable time.
  **Confidence:** low
  **How to verify:** run each locally once and record wall time before adding the job.
- **Claim:** unreferenced integration targets still build.
  **Confidence:** low
  **How to verify:** `cargo test --manifest-path tests/integration/Cargo.toml --no-run`; anything failing is tiered `retired` with a reason, not fixed here.
- **Claim:** `CARGO_LOCKED` is not a cargo environment variable.
  **Confidence:** medium
  **How to verify:** cargo docs for the pinned nightly; fall back to explicit `--locked`.

## Related Code Files

- Create: `scripts/lanes.toml`, `scripts/check-lanes.py`, `scripts/host-test-crates.txt`, `.github/actions/setup/action.yml`
- Modify: `.github/workflows/ci.yml`, `.github/workflows/perf.yml` (drop `continue-on-error` on the C2C lifecycle job or move it), `docs/specs/10-testing.md` §5

## Implementation Steps

1. Run the three candidate x86 lanes locally; record time and result in `reports/phase-02-evidence.md`.
2. Inventory lane scripts and integration targets into `lanes.toml`, tiering from observed results.
3. Write `check-lanes.py` and the host-test crate check; add both to the pre-build guard block.
4. Add the `x86-c2c` job using the existing x86 recipe; add x86-64 workspace clippy with the same excludes as RV64.
5. Add `--locked`, `concurrency`, job timeouts, SHA pins for actions, a pinned `cryptography` requirement.
6. Rewrite Spec 10 §5 from the manifest.

## Todo List

- [ ] Local lane timings
- [ ] `lanes.toml` + guard
- [ ] `x86-c2c` job green
- [ ] Reproducibility settings
- [ ] Spec 10 updated

## Success Criteria

- [ ] `python3 scripts/check-lanes.py` exits 0; adding an unlisted `scripts/qemu-foo.sh` makes it fail (negative control).
- [ ] The `x86-c2c` job passes on `main` and fails when the two-node test is run with a deliberately broken peer (the script's own `C2C_NEGATIVE=1` path reports FAIL as designed).
- [ ] No cargo invocation in `ci.yml` resolves without the lockfile.

## Risk Assessment

- TCG two-node run too slow for PR tier: place it in `nightly` and keep the domain + lifecycle tests on PR; record the choice. Rollback: delete the job and guard; no persistent effect.
- Unpinned-action changes can break workflows: pin one workflow per commit.

## Security Considerations

SHA-pinned actions and a locked dependency graph close the supply-chain drift noted in the security audit (12 S-4, S-5).

## Next Steps

Phase 03 uses the new job to prove the embed-input change.

## Deviation Log

None.
