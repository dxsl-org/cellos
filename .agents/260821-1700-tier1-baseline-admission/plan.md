---
title: "Tier 1 Baseline and Production Admission"
description: "Security-gated rust-no-std baseline, publisher provenance, and owner A/B admission plan; the Phase 01 provenance contract is ready for required approval, while production admission remains disabled pending qualified external-floor evidence."
status: pending
priority: P1
effort: 4w
branch: main
tags: [tier1, admission, provenance, signing, owner-consent, anti-rollback]
created: 2026-08-21
parent: ".agents/260821-0642-app-tiers-completion/phase-03-tier1-baseline-admission.md"
---

# Tier 1 Baseline and Production Admission

## Child Contract

This is the executable child plan for umbrella Phase 03 / TODO C3. It preserves the Phase 01 SDK matrix and produces no claim that current G1/dev admission is production-safe. `rust-no-std` remains the only current Tier 1 baseline; `ffi-posix` and Lua remain trusted runtime profiles, not containment. Tier 1 production admission is disabled until every irreversible gate below is satisfied.

Admission is exactly `valid publisher provenance ∧ authenticated owner authorization at external floor generation`. Path labels, manifest protection class, policy capability ceilings, and a present signature alone are not substitutes for either claim.

These are milestone-local production-admission gates. Per
[ADR-0007](../../docs/decisions/0007-development-first-hardware-constrained-execution.md),
they do not block unrelated QEMU, two-RPi3, incoming-sensor, or local-runtime
development, and none of that development can satisfy or weaken this plan's
production requirements.

## Current-State Findings

- All supported spawn sources converge in `kernel/src/loader.rs::spawn_gated`: `spawn_from_path` supplies boot-table bytes, while `loader/mem_spawn_gate.rs` converts caller names to `/mem/` labels before using that same gate. The latter must receive the same publisher/owner decision and must never inherit path authority.
- Current publisher verification checks a 64-byte `__ViCell_sig` against sorted PT_LOAD bytes plus `__ViCell_manifest` (`kernel/src/signing.rs`). It does not bind a source revision, dependency closure, build recipe, tool binary, or final whole-ELF digest.
- `scripts/cellos-sign` runs the F1 source scan and strict F5 toolchain check before `sign-cell.py` signs an arbitrary supplied ELF and re-verifies it. The controlled CI/KMS pipeline, rather than that CLI alone, must bind checked inputs to the supplied artifact.
- Current anchors are compile-time dev/zero constants in `signing.rs` and `policy.rs`; the current policy is fleet-signed and path-keyed, not owner-signed and digest-keyed. Default G1 permits missing cell signatures. None is a production admission profile.
- The umbrella-approved owner store is a signed, digest-pinned atomic A/B store. Its generation must be anchored outside replaceable A/B media. No qualified floor backend exists in this plan or repository evidence.

## Phases

| # | Phase | Status | Effort | Depends on | May proceed now |
|---|---|---:|---:|---|---|
| 01 | [Freeze baseline and publisher provenance contract](phase-01-freeze-baseline-and-provenance-contract.md) | awaiting required approvals | 4d | Phase 01/02 umbrella evidence | documentation complete; security-owner and independent-reviewer approval required |
| 02 | [Define owner A/B store and qualify the external-floor contract](phase-02-owner-store-and-floor-qualification.md) | blocked | 6d | 01 approvals; candidate backend/evidence | blocked until an actual candidate qualifies |
| 03 | [Integrate provisioned publisher plus owner admission gate](phase-03-provisioned-admission-gate.md) | blocked; core/harness slice complete | 7d | 01 approvals, 02 qualified, security approval | backend-neutral core + non-qualifying harness only; production integration no |
| 04 | [Run hostile evidence, approvals, and ledger closure](phase-04-negative-evidence-and-approval.md) | blocked — prequalification infrastructure complete; admissible evidence blocked | 3d | 03; signed CI or secure measured runner | bounded catalog/parser complete; production evidence no |

## Dependency Graph

`Phase 01 baseline/provenance contract → Phase 02 floor interface + qualified backend → Phase 03 boot-provisioned anchors, store, loader gate → Phase 04 hostile evidence + independent approval → Phase 02 ledger record`.

The documentation-only Phase 01 contract and its evidence are ready for required approval; that is not security-owner or independent-reviewer approval, and it does not qualify a floor. Phase 02 now has an executable backend-neutral decision contract and non-qualifying hostile fake, but may not nominate the fake as a backend or treat emulation/replayable A/B media as qualification. Phase 04 now has a verified and reviewed bounded prequalification catalog/parser, not admissible runtime evidence. Production Phase 03 integration and Phase 04 remain BLOCKED until both Phase 01 approvals are recorded, a real backend passes the qualification contract, and signed CI or a secure measured runner can authenticate retained runtime evidence.

## Core/Harness Completion Evidence

The authorized Core+harness-only sub-slice is **COMPLETE**; child Phases 02–04 remain **BLOCKED**:

- `bash scripts/build-test-hooks-ci.sh` passed exit 0, compiling the RV64 `test-hooks` kernel with `RUSTFLAGS="-D warnings -C relocation-model=pic"`.
- The bare-metal path logged `[INFO] admission-core self-test PASS (fail-closed A/B floor model)`, and the documented QEMU test-hooks runner `cd tests/integration && CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu cargo test --test vfs-quota` passed 1/1. All 31 named admission cases are reachable.
- `cargo build --release --target riscv64gc-unknown-none-elf -Z build-std=core,alloc -p cellos-kernel --no-default-features` passed, as did the same command with `--features policy-required,signing-required`; both exclude the hostile/test modules.
- `cargo test -p types -p api --target x86_64-unknown-linux-gnu` remained at 101 passed, 0 failed, and 4 ignored.
- `AdmissionQualityReview` returned PASS/CORRECT with no findings. `AdmissionSecurityReview` returned PASS with zero Critical, High, or Medium findings.

The two named reviews are independent code/evidence reviews only; they are not human security-owner approval, independent production-design approval, floor qualification, production enablement, Phase 04/release approval, or ledger PASS. The complete command/result record, review scopes, resolved sub-slice blockers, remaining non-waivable blockers, and invalidation rules are in [`phase-03-core-harness-blocker-resolution.md`](phase-03-core-harness-blocker-resolution.md).

## Phase 04 Prequalification Infrastructure Evidence

The bounded Phase 04 prequalification infrastructure is **COMPLETE**; admissible evidence and Phase 04 itself remain **BLOCKED**:

- The byte-pinned catalog covers all 18 mandatory rows and maps all 33 stable compiled `C3-ADM-*` test-hooks IDs. The strict ordered runtime parser remains pure validation logic. The no-argument CLI validates only the canonical catalog and has no capture, runner, custom-input, manifest, bundle, log, or evidence-writer path.
- Final local verification passed: Python 13/13; RV64 all 33 IDs exactly once in canonical order plus aggregate PASS; documented QEMU integration 1/1; both production-shaped builds passed their marker-exclusion checks; host aggregate unchanged at 101 passed, 0 failed, 4 ignored. No runtime log or evidence artifact was retained.
- Final quality review returned correct/KEEPABLE with no findings; final security review returned PASS with no Critical, High, or Medium findings. These agent reviews are not human security-owner approval, independent human production-design approval, release approval, or ledger PASS.
- The former local capture/runner and generated `b7997` bundle were removed. Their same-process self-reported environment, source/kernel origin, backend, and replay claims could not become authentic merely by hashing them.
- Signed CI or a secure measured runner is now an explicit prerequisite before any content-addressed Phase 04 runtime evidence is retained. A qualified floor and physical fault evidence, production parsers/task wiring and anchors, controlled final-ELF provenance, the production profile, both human approvals, release approval, and Phase 02 validation still remain. The ledger is unchanged.

## Phase 01 Evidence and Approval State

- Proposed contract: [`docs/specs/18c-publisher-provenance-envelope.md`](../../docs/specs/18c-publisher-provenance-envelope.md). It defines the version-1 detached envelope and controlled CI/KMS handoff; it introduces no producer, parser, production profile, or admission path.
- Host aggregate baseline evidence: `cargo test -p types -p api --target x86_64-unknown-linux-gnu` completed with 101 passed, 0 failed, and 4 ignored.
- Contract review found two specification defects; both were corrected, and a focused recheck passed. A final independent document review found no blocking document defect.
- This evidence makes the proposed design **ready for**—not approved by—the required security owner and independent reviewer. Neither review result substitutes for either approval.
- Non-waivable remaining gates are: recorded security-owner approval; recorded approval by an independent reviewer who did not author the design; a real external floor that meets every qualification clause with actual rollback/replay and power-loss evidence; acceptance of that floor evidence by both approvers; then the separately gated implementation and hostile-evidence/release approvals below.

## Exact Provenance Boundary

The implementation must add a publisher-signed, canonical provenance envelope created only after the final signed ELF exists. It must bind:

1. `SHA-256(final_elf_bytes)` — the exact digest used by owner admission and measurement; this avoids treating the current payload-only signature as a whole-file identity.
2. Current signature algorithm/key identifier and a digest of its canonical payload: sorted `(PT_LOAD.p_offset, p_filesz, phdr_index, bytes)` plus `__ViCell_manifest`, matching `scripts/sign-cell.py::_pt_load_payload` and `kernel/src/signing.rs::verify_cell_with_key`.
3. Source identity: immutable VCS revision, clean/dirty state, and a canonical tracked-input manifest covering each Cell crate root, tracked Rust inputs under `cells/`, `scripts/unsafe-allowlist.toml`, and the F1-check result.
4. Dependency/toolchain identity: `Cargo.lock` digest where applicable, `rust-toolchain.toml` digest and resolved compiler identity, target triple/target JSON, Cargo packages, profile, feature set, linker/objcopy identity, and an allowlisted build environment/recipe digest.
5. CI/KMS signer identity, signing timestamp/nonce or build invocation identity, and the detached publisher signature over the canonical envelope.

The envelope MUST be detached from the ELF so `final_elf_bytes` has no self-referential hash. The install bundle and owner store retain the envelope and publisher signature; kernel verification MUST validate them against the actual loaded whole-ELF digest before owner lookup. The existing `__ViCell_sig` remains the execution-content/manifest integrity check until a separately approved format migration replaces it. `--unchecked-dev-signature`, reproducible dev keys, raw CLI target paths, and a provenance envelope authored outside controlled CI/KMS cannot satisfy production Claim A.

## External-Floor Qualification Contract

No backend is selected here. A candidate must implement a boot-usable, authenticated `read` plus conditional `advance` contract over `(generation, transaction_id, transaction_intent_digest)`:

- `read` returns an authenticated, durable floor state; a caller cannot replay an older observation or substitute an unauthenticated one.
- `advance(expected_generation, transaction_id, intent_digest)` atomically
  compares `expected_generation`, binds the supplied `transaction_id` and
  `intent_digest`, durably advances exactly one generation, and returns
  evidence that cannot be forged or rolled back by replacing either A/B slot.
- Repeating an already-completed request is identifiable and cannot create a second advancement; conflicting intent at the same generation fails closed.
- Power loss at every point has a specified recovery observation. If the floor is ahead of both slots, or a slot is ahead of the floor, recovery is authenticated and explicit; it never derives or advances the floor from a slot.
- The trust path, provisioning, authorization, persistence/failure domain, counter exhaustion/reset behavior, and removal/replacement attacks are documented and exercised on the actual candidate. A bare counter is insufficient unless it provides an authenticated atomic intent binding with equivalent rollback resistance.

Qualification evidence must include independent power-loss/torn-write and rollback/replay drills against the physical or otherwise non-replayable backend. A filesystem file, VIFS1 blob, normal disk sector, TPM/NVRAM claim without the stated atomic intent semantics, or a test double is not qualification.

## Irreversible Gates and Blockers

1. **Design gate (before Phase 03):** security owner approves the publisher/owner separation, exact provenance encoding, A/B transaction/recovery state machine, and threat model; an independent reviewer who did not author the design approves it separately.
2. **Floor gate (before Phase 03):** platform custodian supplies the candidate's qualification report, fault-injection evidence, provisioning/reset procedure, and signed identity binding. Security owner and independent reviewer accept that evidence. No approval may waive the non-replayable-floor requirement.
3. **Implementation gate (before production feature enablement):** boot-provisioned publisher and owner anchors replace the dev/zero constants; named production feature/profile enables mandatory publisher and owner admission; weak/dev key routes are absent; controlled CI/KMS produces the provenance envelope; all spawn entry points are covered.
4. **Release gate (before ledger PASS):** signed CI or a secure measured runner authenticates the retained runtime evidence; the Phase 04 hostile suite passes against the production profile and qualified floor; security owner signs the threat/evidence package; a reviewer independent of the implementer signs the review; Phase 02 records content-addressed PASS evidence. Any anchor, policy, provenance format, runner trust base, floor backend, recovery algorithm, or loader-gate change invalidates approval and requires rerun.

Current non-waivable blockers: there is no qualified non-replayable external floor or physical replay/power-loss qualification package; no approved owner-store, provenance, or floor parser/persistence path; no provisioned owner anchor; no provisioned publisher anchor; no production profile or controlled CI-to-final-ELF provenance record; no common loader/task-creation runtime wiring and no-task-on-denial proof; no signed CI or secure measured runner able to authenticate and retain admissible Phase 04 runtime evidence; no required human approvals; and no Phase 04 hostile/release evidence or governed ledger PASS. The completed Core+harness-only and prequalification-infrastructure slices satisfy none of those physical, production-integration, evidence-authentication, governance, or human gates. Therefore production admission MUST remain disabled and the Phase 02 ledger row MUST remain BLOCKED.

## Planned File Ownership

- Provenance producer and controlled build integration: `scripts/cellos_sign/cli.py`, `scripts/cellos_sign/signing.py`, `scripts/sign-cell.py`, `scripts/lib-sign-cells.sh`, and each signing image lane such as `scripts/build-boot-ramdisk-ci.sh`.
- Kernel trust/admission: internal state core and non-qualifying tests under `kernel/src/admission/`; future blocked production integration in `kernel/src/signing.rs`, `kernel/src/loader.rs`, `kernel/src/loader/mem_spawn_gate.rs`, `kernel/src/main.rs`, and `kernel/src/audit.rs`; `kernel/src/policy.rs` remains a verify-then-parse convention only (do not extend `/POLICY.BIN`).
- Test/evidence seams: `scripts/test_cellos_sign.py`, `scripts/test-cell-signing.sh`, `kernel/src/loader/elf_tests.rs`, new focused admission-store/floor fake tests, production-profile build configuration, and `docs/app-tier-acceptance-ledger.json` through Phase 02 governance.

## Non-negotiables

- Owner consent only narrows a valid publisher decision; it never authorizes unsigned, wrong-key, tampered, unprovenanced, or stale-digest bytes.
- The owner record keys the whole-file SHA-256, not a mutable pathname. The provenance envelope must be publisher-verified against the same whole-file digest.
- A/B slots are replaceable cache/state, never the generation floor. The loader admits only an authenticated, committed slot whose generation and intent binding match the authenticated external floor.
- Floor-ahead, slot-ahead, malformed, missing, ambiguous, and recovery-error states deny admission with audit evidence. They create no task and select no fallback/highest slot.
- Development posture is a separately named reboot/build choice. It must not be described or configured as production admission.
