---
title: "Phase 03 Core and Hostile Harness Blocker Resolution"
status: complete
scope: "backend-neutral core and non-qualifying test harness only"
production_status: blocked
date: 2026-08-21
---

# Phase 03 Core and Hostile Harness Blocker Resolution

## Decision and Safety Boundary

The approved resolution is **Core + harness only**. The internal kernel module models authenticated floor observations, authenticated A/B slot observations, and a pure admission/recovery decision. A deterministic in-memory floor and transaction model exists only under `test-hooks` and is explicitly named and documented as non-qualifying.

Production admission remains disabled. This slice does not add a backend, anchors, parsers, signatures, provenance production, owner-store loading, audit claims, loader calls, task-creation calls, boot initialization, developer fallback, or a production feature/profile. `spawn_gated`, `spawn_from_mem_gated`, signing, policy loading, measurement, and task creation are unchanged. The only runtime invocation is the existing ELF power-on self-test path when `test-hooks` is enabled.

The pure result has only `Admit`, `Deny`, and `RecoveryRequired`; it has no floor-advance result. Admission requires exactly one authenticated committed slot equal to all four authenticated floor fields—generation, transaction ID, intent digest, and backend identity—plus one authenticated stale committed partner from that same backend. Missing, invalid, uncommitted, ambiguous, conflicting, floor-ahead, slot-ahead, and backend-failure observations fail closed. Neither slot is selected because it is numerically highest.

## Repository Findings

- `kernel/src/loader.rs::spawn_gated` remains the common byte-admission path, and `kernel/src/loader/mem_spawn_gate.rs` feeds it a neutral `/mem/` label. Neither file is touched by this slice.
- The kernel already runs plain compiled boolean self-tests before the first non-TCB spawn. The admission harness is declared internally and invoked there only under `feature = "test-hooks"`.
- The existing publisher-signature and fleet-policy paths use development/compile-time material and do not implement authenticated owner-floor admission. They were not modified or relabeled.
- The local A/B media and writable image are in the same replaceable failure domain and therefore cannot supply the external floor.
- The available Raspberry Pi 3 is not covered by Raspberry Pi's documented secure-boot flow, which is described for Raspberry Pi 4 or newer. Its documented OTP storage is finite, one-time programmable storage, not the required reusable authenticated conditional transaction service.

## Primary-Source Assessment

These sources constrain candidate selection; none is recorded as qualified evidence:

1. Raspberry Pi OTP register definitions and customer OTP rows: <https://github.com/raspberrypi/documentation/blob/c197720806999f5bf9f675ccc324d5a5a1cadca4/documentation/asciidoc/computers/raspberry-pi/otp-bits.adoc#L1-L66> and the industrial/customer-write rows at <https://github.com/raspberrypi/documentation/blob/c197720806999f5bf9f675ccc324d5a5a1cadca4/documentation/asciidoc/computers/raspberry-pi/otp-bits.adoc#L48-L98>. These establish fixed OTP facilities, not the required atomic compare-and-bind floor API.
2. Raspberry Pi secure boot: <https://github.com/raspberrypi/usbboot/blob/master/docs/secure-boot.md>. It documents Raspberry Pi 4-or-newer verified boot and irreversible OTP provisioning, not a Pi 3 authenticated mutable intent floor.
3. TPM reference `NV_Increment`: <https://github.com/microsoft/ms-tpm-20-ref/blob/ee21db0a941decd3cac67925ea3310873af60ab3/TPMCmd/tpm/src/command/NVStorage/NV_Increment.c#L14-L63>. The reference increments a counter after authorization/access checks; it does not by itself establish atomic expected-generation comparison plus transaction-ID and intent-digest binding.
4. TPM reference `NV_Extend`: <https://github.com/microsoft/ms-tpm-20-ref/blob/ee21db0a941decd3cac67925ea3310873af60ab3/TPMCmd/tpm/src/command/NVStorage/NV_Extend.c#L18-L70>. The reference hashes old digest with supplied data and writes the result; qualification would still need authenticated boot reads, exactly-once conflict semantics, durability, freshness, and the complete failure-domain evidence.
5. Certificate Transparency v2: <https://www.rfc-editor.org/rfc/rfc9162.html>. Its append-only Merkle-log and signed-tree-head mechanisms are useful candidate references, but no concrete boot-available service, custody path, freshness rule, or physical failure evidence has been supplied.
6. TLS 1.3: <https://www.rfc-editor.org/rfc/rfc8446.html>. Authenticated transport can protect a future remote-floor protocol, but transport authentication alone does not prove non-rollback durable server state or the required atomic conditional transaction.

Therefore the current Raspberry Pi 3 cannot qualify a production floor, and naming OTP, TPM NV, an append-only log, or TLS does not satisfy the qualification contract.

## Compiled Bare-Metal Test Identifiers

State matrix in `kernel/src/admission/state_selftest.rs`:

- `old_a_replay_admits_current_b_only`
- `old_b_replay_admits_current_a_only`
- `both_old_slots_deny`
- `stale_floor_response_denies`
- `wrong_transaction_binding_denies`
- `wrong_backend_binding_denies`
- `torn_uncommitted_slot_denies`
- `missing_slot_denies`
- `invalid_slot_denies`
- `floor_ahead_denies`
- `slot_ahead_denies`
- `duplicate_current_slots_deny_as_ambiguous`
- `missing_backend_denies`
- `invalid_backend_evidence_denies`
- `replaced_backend_denies`
- `unavailable_backend_denies`
- `exhausted_backend_denies`

Transaction/failure matrix in `kernel/src/admission/transaction_selftest.rs`:

- `power_loss_before_intent_write`
- `power_loss_after_intent_write`
- `power_loss_after_intent_verify`
- `power_loss_before_floor_advance`
- `power_loss_after_floor_advance`
- `power_loss_before_commit_write`
- `power_loss_after_commit_write`
- `power_loss_after_commit_verify`
- `duplicate_advance_is_exactly_once`
- `conflicting_advance_fails_closed`
- `wrong_expected_generation_fails_closed`
- `unavailable_advance_fails_closed`
- `exhausted_advance_fails_closed`
- `local_history_cannot_admit_or_advance_floor`

The boundary tests pin zero successful advancement before the floor-advance boundary, exactly one afterward, deny/recovery for uncommitted split transactions, and admission only after a committed current slot has an authenticated stale partner. The local-history test asserts both no admission and unchanged fake advance counters.

## Final Verification Evidence

The authorized Core+harness-only sub-slice is **COMPLETE**. The following commands and results close only its compile, reachability, execution, isolation, and review blockers:

- `bash scripts/build-test-hooks-ci.sh` exited 0. The script built the RV64 kernel with `test-hooks` and `RUSTFLAGS="-D warnings -C relocation-model=pic"`.
- `BOOT_WINDOW=55 bash scripts/qemu-boot-test.sh target/riscv64gc-unknown-none-elf/release/cellos-kernel-test-hooks` reached the bare-metal log `[INFO] admission-core self-test PASS (fail-closed A/B floor model)`. The wrapper then exited 1 solely at its deliberate stack probe, so this probe is retained as log evidence rather than represented as a clean runner result.
- The documented QEMU test-hooks runner, `cd tests/integration && CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu cargo test --test vfs-quota`, passed 1 test, with 0 failed and 0 ignored.
- All 31 named admission tests are reachable: 17 state cases and 14 transaction/failure cases. Both run groups and every case use non-short-circuit boolean AND, so the PASS log covers every named case rather than a short-circuited prefix.
- `cargo build --release --target riscv64gc-unknown-none-elf -Z build-std=core,alloc -p cellos-kernel --no-default-features` passed while excluding the hostile/test modules.
- `cargo build --release --target riscv64gc-unknown-none-elf -Z build-std=core,alloc -p cellos-kernel --no-default-features --features policy-required,signing-required` passed while excluding the hostile/test modules.
- `cargo test -p types -p api --target x86_64-unknown-linux-gnu` retained the host baseline: 101 passed, 0 failed, and 4 ignored.

## Independent Review Evidence

- `AdmissionQualityReview` independently reviewed the final backend-neutral core/harness diff and plan/evidence updates. Result: **PASS/CORRECT**, confidence 0.96, with no findings. Its scope included state classification, replay/ambiguity handling, crash-boundary fake semantics, outcome consumers, all 31 tests' reachability, production isolation, maintainability, and blocked-status wording.
- `AdmissionSecurityReview` independently performed a static security review of `kernel/src/admission/{mod.rs,hostile.rs,state_selftest.rs,transaction_selftest.rs}`, the minimal `kernel/src/main.rs` invocation, the feature definition, and the Phase 02–04 evidence/status artifacts. Result: **PASS** for the intentionally Core+harness-only slice, with zero Critical, High, or Medium findings.

These are independent code/evidence reviews only. Neither reviewer is recorded as the human security owner, neither result is approval of the production design or implementation, and neither qualifies an external-floor backend, authorizes production, closes Phase 04, or permits ledger PASS.

## Resolved Blockers Within This Slice

- The backend-neutral decision contract compiles warning-free in the RV64 `test-hooks` lane.
- The hostile state and transaction matrices are all reachable and execute to the bare-metal PASS log.
- No-default and `policy-required,signing-required` production-shaped builds compile without including the hostile/test modules.
- Independent quality and security review found no blocker in the bounded Core+harness-only implementation.

No production blocker is resolved by those results. In particular, they provide neither a qualifying floor nor a parser, provisioned anchor, persistence layer, production runtime consumer, physical fault evidence, human approval, or ledger witness.

## Provenance Review Evidence Is Not Approval

The Phase 01 provenance-contract review found two document defects, the defects were corrected, a focused recheck passed, and a final independent document review reported no blocking document defect. This is review evidence only. It is **not** recorded security-owner approval, **not** the required independent non-author approval of the production design, and **not** acceptance of an external-floor backend.

## Remaining Physical and Human Gates

Production integration and any ledger PASS remain blocked until all of the following are retained as content-addressed evidence:

1. A real boot-available backend with a signed/authenticated identity and a failure domain independent of A/B slots, normal storage, VIFS1, and the writable image.
2. Authenticated fresh `read` and atomic conditional `advance(expected_generation, transaction_id, intent_digest)` with durable success, idempotently recognizable duplicates, conflicting-intent rejection, and no slot-derived advancement.
3. Physical or equivalently non-replayable drills for old A, old B, both old, stale reads/responses, torn writes, every protocol power boundary, floor ahead, slot ahead, missing/removal/replacement, unavailability, exhaustion, reset/reprovision, and recovery.
4. Provisioning, authorization, custody, backend identity/key rotation, exhaustion, replacement, and authorized reset procedures exercised on the actual candidate.
5. Boot-provisioned owner and publisher anchors; approved owner-slot and provenance encodings; controlled CI/KMS final-ELF provenance production; and a named production profile with no weak/dev fallback.
6. Recorded security-owner approval of the provenance/owner separation, state machine, threat model, candidate evidence, implementation, and final hostile package.
7. Separate recorded approval by an independent reviewer who did not author the design or implementation, including acceptance of the physical backend evidence.
8. Phase 04 executed evidence and release approval, followed only then by the governed Phase 02 ledger update.

The sub-slice status is **COMPLETE**; production integration and the acceptance-ledger status remain **BLOCKED**. No production acceptance-ledger status is changed by this report.

## Evidence Invalidation Rules

Rerun the exact build, runner, reachability, baseline, quality-review, and security-review evidence above after any change to admission outcome semantics, tuple matching, ambiguity/recovery classification, the hostile fake, any named case or run-group aggregation, `test-hooks` invocation or feature gating, the test-hooks build script/RUSTFLAGS, or the production feature combinations used to prove exclusion. Such a change makes this sub-slice evidence stale; it never makes production admissible by default.

Separately, any change to keys or anchor provisioning, producer/provenance or owner-slot encoding, signing payload rules, floor backend/firmware, persistence or recovery behavior, the common loader/task-creation gate, audit semantics, or the named production profile invalidates the eventual production evidence and approvals and requires the complete physical/hostile/release package to be rerun.
