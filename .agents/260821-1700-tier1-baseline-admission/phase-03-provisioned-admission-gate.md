---
title: "Phase 03 - Integrate Provisioned Publisher and Owner Admission Gate"
status: blocked
core_harness_slice: complete
priority: P1
effort: 7d
depends_on: ["phase-01 approved", "phase-02 qualified external floor", "security-owner approval", "independent-reviewer approval"]
owner: "kernel admission"
---

# Phase 03 - Integrate Provisioned Publisher and Owner Admission Gate

## Context Links

- Parent implementation links: `.agents/260821-0642-app-tiers-completion/phase-03-tier1-baseline-admission.md:21-25`
- Publisher verifier: `kernel/src/signing.rs:1-117`
- Common loader gate: `kernel/src/loader.rs:104-203`
- Memory-origin gate: `kernel/src/loader/mem_spawn_gate.rs:1-64`
- Boot ordering: `kernel/src/main.rs:670-687,870-877`
- Existing verified-store discipline: `kernel/src/policy.rs:145-213`
- Current producer: `scripts/cellos_sign/cli.py:79-105`; `scripts/cellos_sign/signing.py:72-122`; `scripts/sign-cell.py:231-285`

## Overview

Implement the approved production admission path only after Phase 02 qualifies a real external floor and the named approvers sign the design. Replace compile-time dev/zero trust placeholders with boot-provisioned publisher and owner anchors, load the authenticated owner store before any untrusted cell spawn, and make the shared loader enforce publisher provenance and matching owner admission for all spawn sources.

## Authorized Blocker-Resolution Slice

The approved interim strategy is **Core + harness only**. The backend-neutral decision core and explicitly non-qualifying hostile fake are implemented under `kernel/src/admission/`, with a `test-hooks`-only invocation in the existing pre-spawn ELF self-test path. They are not wired into signing, policy loading, boot initialization, `spawn_gated`, measurement, audit, or task creation. This supplies an executable state-machine contract without selecting or simulating a production backend.

Evidence and remaining gates are recorded in [`phase-03-core-harness-blocker-resolution.md`](phase-03-core-harness-blocker-resolution.md). Phase 03 remains blocked; this slice is not external-floor qualification, human approval, production enablement, or ledger PASS.

## Core/Harness Final Evidence

The bounded sub-slice is complete: `bash scripts/build-test-hooks-ci.sh` passed with the RV64 `test-hooks` build enforcing `RUSTFLAGS="-D warnings -C relocation-model=pic"`; the bare-metal path logged `[INFO] admission-core self-test PASS (fail-closed A/B floor model)`; the documented QEMU test-hooks runner passed 1/1; all 31 named cases are reachable; no-default and `policy-required,signing-required` production-shaped builds passed while excluding the hostile/test modules; and the host baseline remained 101 passed, 0 failed, and 4 ignored.

`AdmissionQualityReview` returned PASS/CORRECT with no findings, and `AdmissionSecurityReview` returned PASS with zero Critical, High, or Medium findings. These are independent code/evidence reviews of the bounded sub-slice only—not security-owner approval, independent production-design approval, floor qualification, Phase 04/release approval, production enablement, or ledger PASS. Exact commands, results, scopes, resolved blockers, remaining blockers, and invalidation rules are retained in [`phase-03-core-harness-blocker-resolution.md`](phase-03-core-harness-blocker-resolution.md).

## Key Insights

- `spawn_gated` is the only code path to modify for common byte admission. `spawn_from_path` and `spawn_from_mem_gated` already converge there; a second gate would drift and permit a bypass.
- `main.rs` presently loads policy after VIFS1 initialization and before cap-bearing cell spawns, but it spawns `init` directly with `task::spawn_from_mem`, bypassing `spawn_gated`. The design must explicitly classify kernel-root boot artifacts: they are TCB boot inputs, not caller-admitted cells, and this exemption must stay narrow, documented, and non-exported.
- Measurement currently follows task creation at `loader.rs:195-203`. The whole-ELF SHA-256 needed by the owner lookup must be computed once before task creation, passed to admission/measurement, and not recomputed unnecessarily.

## Requirements

- Extend the internal `kernel/src/admission/` core only after the blockers clear, adding bounded verify-then-parse loaders for the provisioned publisher/owner anchors, detached publisher provenance envelope, external-floor evidence, and A/B owner store. Do not extend `/POLICY.BIN` or reuse the fleet policy key.
- Refactor `kernel/src/signing.rs` to consume the provisioned publisher anchor and retain explicit dev-only test fixtures behind non-production configuration. The production profile must reject dev keys, zero placeholders, weak RNG, missing/invalid signature, missing/invalid provenance, and unprovisioned anchors.
- Make `kernel/src/main.rs` initialize anchors, external-floor adapter, and owner store after storage is ready and before every non-TCB cell spawn. Initialization uncertainty/invalidity means production admission stays disabled/denies, not dev fallback.
- At the start of `spawn_gated`, compute the full ELF digest once, verify existing publisher payload signature, verify detached provenance against that digest, then consult the owner store under external-floor evidence. Only this conjunction proceeds to manifest parsing or `task::spawn_from_mem`.
- Preserve `mem_spawn_gate` label sanitization and use the same digest/provenance/owner lookup for `SpawnFromMem`; label/path cannot select owner consent.
- Add distinct audit events for publisher-provenance failure, owner missing/denied, invalid store/floor, recovery-required mismatch, and owner-admitted spawn. Events contain non-secret reason codes/digests only.
- Supply controlled CI/KMS producer integration in `scripts/cellos_sign/cli.py`, `scripts/cellos_sign/signing.py`, `scripts/sign-cell.py`, `scripts/lib-sign-cells.sh`, and every relevant image lane. No generic developer CLI mode may mint production envelope/signature material.

## Architecture

`boot-provisioned publisher key + owner key + qualified floor adapter → verified owner A/B state` at boot.

For each non-TCB spawn:

`ELF bytes → SHA-256 once → existing payload signature → detached publisher provenance(final ELF hash) → owner record(digest, provenance digest) in committed slot == authenticated floor → manifest/path capability gates → task creation → measurement using precomputed digest`.

A false/missing/error result at any arrow returns `PermissionDenied` before task creation. Capability policy remains a later narrowing operation; it cannot rescue admission.

## Related Code Files

- `kernel/src/admission/mod.rs`
- `kernel/src/admission/hostile.rs` (`test-hooks` only; explicitly non-qualifying)
- `kernel/src/admission/state_selftest.rs` (`test-hooks` only)
- `kernel/src/admission/transaction_selftest.rs` (`test-hooks` only)
- `kernel/src/signing.rs` (future production integration; unchanged by the core slice)
- `kernel/src/loader.rs`
- `kernel/src/loader/mem_spawn_gate.rs`
- `kernel/src/main.rs`
- `kernel/src/measurement_log.rs`
- `kernel/src/audit.rs`
- `kernel/src/policy.rs` (convention/reference only; no owner-store extension)
- `kernel/Cargo.toml` and named production image/profile configuration
- `scripts/cellos_sign/cli.py`, `scripts/cellos_sign/signing.py`, `scripts/sign-cell.py`, `scripts/lib-sign-cells.sh`
- `scripts/build-boot-ramdisk-ci.sh`, `scripts/build-shell-test-ci.sh`, `scripts/build-srv-test-ci.sh`, `scripts/build-test-hooks-ci.sh`

## Implementation Steps

1. Land provisioned-anchor format/loader and explicit production configuration validation. Remove any path in that profile that falls back to a dev key, zero key, unsigned acceptance, or an absent owner store.
2. Implement canonical provenance envelope verification and slot/floor parser from the Phase 01/02 contracts, including bounded lengths and verify-before-parse ordering.
3. Implement external-floor adapter behind the Phase 02 qualified contract; bind its authenticated result to slot transaction intent/commit validation and recovery-required outcomes.
4. Compute the full ELF digest once in `spawn_gated`; pass it through publisher provenance, owner lookup, and measurement API. Do not allocate/copy the ELF merely to hash it.
5. Replace the current signature-only branch with the ordered conjunction and add audit events. Ensure every error returns before `spawn_from_mem` and schedules no task.
6. Wire the same dependency into path and memory-origin spawns; retain `/mem/` path neutralization for post-admission capability decisions.
7. Update all signing image lanes to produce, package, and verify final-ELF provenance envelopes under CI/KMS. Add no developer fallback that labels local artifacts production.
8. Conduct security-owner and independent-reviewer review of the final diff and state-machine traces before enabling the named production configuration.

## Todo List

- [x] Backend-neutral decision core and non-qualifying `test-hooks` harness implemented, executed, and independently reviewed.
- [ ] Qualified external-floor adapter and evidence digest supplied.
- [ ] Boot-provisioned publisher and owner anchors loaded before non-TCB spawns.
- [ ] Detached provenance producer/verifier deployed in every production image lane.
- [ ] Common gate denies before task creation on every failed Claim A/Claim B branch.
- [ ] Full-ELF digest is computed once and reused for measurement.
- [ ] Audit taxonomy and TCB init exemption reviewed.
- [ ] Security owner and independent reviewer approve implementation.

## Acceptance Criteria

- In the named production profile, no spawn source can enter SAS without valid current payload signature, valid publisher provenance for its exact whole ELF, and a committed owner record matching the authenticated external floor.
- Replacing ELF bytes, provenance envelope, either slot, both slots, owner key, publisher key, or floor evidence fails closed before task creation.
- `SpawnFromMem` cannot bypass owner admission or forge path-derived authority; `/bin` classification remains independent of provenance/consent.
- Dev/default behavior remains explicitly separate and cannot be accidentally selected by a production configuration.

## Risk Assessment

- **Boot deadlock/lockout:** missing or invalid provisioned data prevents cells from starting. Mitigation: production behavior deliberately fails closed; recovery/reprovision is a separately authenticated procedure.
- **Init bypass expands:** direct boot spawn could become a general exemption. Mitigation: retain it solely for fixed kernel TCB boot artifact, do not expose it through syscalls, and audit all direct `task::spawn_from_mem` callsites.
- **Digest divergence:** admission and measurement identify different bytes. Mitigation: calculate one whole-ELF digest before task creation and pass it as data to both consumers.

## Security Considerations

Publisher provenance and owner authorization are separate keys, parsers, audit outcomes, and failure modes. A valid owner signature cannot forgive publisher failure; a valid publisher signature cannot admit without owner consent. All uncertain store/floor state is denial/recovery-required, never an availability fallback.

## Rollback

Disable the named production configuration and return to explicitly labeled development behavior; do not delete/reuse floor generations or roll back a store to make old artifacts admissible. Production rollback requires an approved reprovisioning procedure and invalidates prior release evidence.

## Next Steps

Phase 04 executes hostile negative evidence, power-loss/replay drills, approval review, and Phase 02 ledger recording. Production enablement remains blocked until Phase 04 PASS.

## Deviation Log

- 2026-08-21: With production integration blocked, the approved Core+harness-only slice implemented the pure state contract and hostile bare-metal tests without a backend or loader/boot wiring. See `phase-03-core-harness-blocker-resolution.md`. All original physical, provisioning, implementation, approval, and release gates remain open.
