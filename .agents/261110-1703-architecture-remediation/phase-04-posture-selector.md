---
phase: 4
title: "One trust posture, guarded copy always on"
status: completed
priority: P1
effort: "5–7d"
dependencies: [3]
tier: thinking
checkpoint: false
---

# Phase 04: One trust posture, guarded copy always on

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Context Links

[Review H-4](../reports/architecture-review-261110.md) · `kernel/Cargo.toml` · `kernel/build.rs` · `kernel/src/main.rs` · `kernel/src/signing.rs` · `kernel/src/policy.rs` · `kernel/src/admission/` · `kernel/src/task/user_copy/` · [Red team](reports/red-team.md) #7

## Overview

Replace additive posture features with exactly one derived posture, make the guarded user-copy boundary unconditional on 64-bit MMU targets, and boot the release posture once in CI.

## Key Insights

- Verified in `kernel/Cargo.toml`: `default` is the dev posture; `release-posture` and `production-relay-image` are feature bundles; the manifest itself states that without `native-domains` "the user-copy boundary is a raw memcpy".
- Reviewer-cited: only `production-relay-image` rejects dev/test features (`main.rs:17-29`); policy key is a zero placeholder off-dev (`policy.rs:115-118`); owner gate is permanently permissive (`admission/gate.rs:24,59-62`); release features appear only in build/check steps.
- Cargo feature unification makes cross-class mixes a realistic accident.

## Requirements

- Functional: `build.rs` emits one of `cfg(posture = "dev" | "test" | "release" | "relay")` and fails the build on any cross-class combination (e.g. release + `test-hooks`, release + `dev-policy-key`, release + `maintenance-mode`); a provisioned policy public key for release, parallel to `CELLOS_RELEASE_CELL_PUBLIC_KEY`; owner gate either enforced under release or removed from the spawn path; guarded copy path is the only path on riscv64/aarch64/x86_64.
- Non-functional: existing feature names keep working as inputs; dev lanes unchanged.

## Architecture

- Posture derivation table in `build.rs` (inputs: existing features + key env vars → one cfg). Code switches from `cfg(feature = "dev-…")` to `cfg(posture = …)` only at the trust-anchor and gate sites.
- `copy_glue`: delete the `Shared` raw-memcpy variant on 64-bit MMU targets; 32-bit legacy targets keep it until owner decision 4, and cannot be built with a release posture (`compile_error!`).
- CI: one `x86-release-posture-boot` lane using a CI-only throwaway key pair generated in the job.

## Assumptions

- **Claim:** under `release-posture`, non-core paths receive `CapSet::EMPTY` and drivers cannot start, so the release posture has never booted end to end.
  **Confidence:** low (reviewer inference)
  **How to verify:** the new boot lane; if it fails, the failure is the finding and is fixed in Phase 05 (catalog) rather than papered over here.
- **Claim:** every 64-bit lane already enables `native-domains` (it is in `default`).
  **Confidence:** medium
  **How to verify:** grep `--no-default-features` in `scripts/` and `.github/`; list feature sets lacking it.
- **Claim:** 180 `native-domains` cfg sites can be collapsed mechanically.
  **Confidence:** medium
  **How to verify:** `grep -rn 'native-domains' kernel/src | wc -l` and sample 10.

## Related Code Files

- Modify: `kernel/build.rs`, `kernel/Cargo.toml`, `kernel/src/main.rs`, `kernel/src/signing.rs`, `kernel/src/policy.rs`, `kernel/src/admission/{gate,mod,owner_key}.rs`, `kernel/src/loader/governed_spawn.rs`, `kernel/src/task/copy_glue/mod.rs`, `.github/workflows/ci.yml`, `scripts/lanes.toml`, `docs/security-model.md`
- Create: `kernel/src/posture.rs` (constants + doc of the table)

## Implementation Steps

1. Enumerate current feature sets used by every lane (from `lanes.toml`); write the derivation table; get it reviewed in the phase evidence file.
2. Implement derivation and the mix rejection in `build.rs`; add host tests for the table (pure function).
3. Switch trust-anchor, policy-key, RNG-fallback and maintenance sites to `cfg(posture)`.
4. Policy key: add `CELLOS_RELEASE_POLICY_PUBLIC_KEY`, refuse zero/dev values exactly as the cell key does.
5. Owner gate: enforce under release/relay or delete the call at `governed_spawn.rs:117`; decision recorded.
6. Remove the non-guarded copy path on 64-bit targets; delete `validate_user_buf` callers that the copy view replaces (list each).
7. Add the release-posture boot lane.

## Todo List

- [x] Derivation table reviewed
- [x] Mix rejection + tests
- [x] Policy key provisioning
- [x] Owner gate decision applied
- [x] Guarded copy unconditional
- [x] Release posture boot lane

## Success Criteria

- [x] `cargo check -p cellos-kernel --no-default-features --features release-posture,test-hooks` fails with the posture error (repeat for `dev-policy-key`, `maintenance-mode`).
- [x] Host tests for the derivation table pass.
- [x] x86 release-posture image boots to the shell marker in QEMU, or the lane records the exact refusal as input to Phase 05. (Records the refusal: every Cell refused, init stops at the VFS bootstrap.)
- [x] RV64 baseline and an AArch64 boot lane still pass with default features.

## Risk Assessment

- A dev lane relied on a mixed set: the build error names it; fix the lane's feature list. Rollback: revert; feature-flag changes only.
- Changing anchors invalidates already-signed artifacts and policies: only new release builds are affected; nothing released under the old scheme is re-validated.

## Security Considerations

This phase is the structural fix behind audit H-9/H-10. The CI key pair is generated per job and never stored; no real release key enters the repository.

## Next Steps

Phase 05 consumes the posture cfg for fixture rows.

## Deviation Log

- Decision: posture is a `cfg(cellos_posture)` derived from the existing features by `kernel/src/posture.rs` (shared by `build.rs` and host tests); feature names stay the inputs.
  Why: one tested definition for both enforcement and code.
  Impact: `kernel/build.rs`, `kernel/src/posture.rs`.
  Revert: drop the cfg; restore placeholders.
- Decision: owner gate enforced under release (owner `enforce-release`); the owner and policy anchors are provisioned like the cell anchor.
  Why: owner answer; a zero placeholder in a release kernel was the finding.
  Impact: release kernels now refuse every governed spawn until release-signed cells and an admitted owner slot exist.
  Revert: `production_enforced: false`.
- Deviation: the raw-memcpy copy path is not deleted; the build refuses a bootable 64-bit kernel without `native-domains`.
  Why: 32-bit targets still use it (owner decision D4).
  Impact: none on 64-bit.
  Revert: n/a.
- Deviation: `validate_user_buf` kept.
  Why: all 43 sites reviewed — each precedes copy-view access; it is an argument pre-check, not a boundary.
  Impact: none.
  Revert: n/a.
- Deviation: files outside the S2 list touched as direct consequences: `.github/workflows/release.yml` (provision the two new anchors; S1 file), `scripts/qemu-getrandom-sas-test.sh` (its production tuple now needs three anchors and is compile-only), new lane `scripts/x86/qemu-release-posture-test.sh`, `docs/code-metrics.generated.md`.
  Why: without them release and the getrandom lane break on the new key requirement.
  Impact: listed files.
  Revert: revert with the phase.
- Surprise: the embedded `init` is published as trusted without passing the governed spawn, so a release kernel starts init and refuses everything init asks for.
  Why: observed in the release boot log.
  Impact: the lane asserts that exact shape; whether init itself should pass the signature/owner gate is input for Phase 06.
  Revert: n/a.

## Evidence

[reports/phase-04-evidence.md](reports/phase-04-evidence.md).
