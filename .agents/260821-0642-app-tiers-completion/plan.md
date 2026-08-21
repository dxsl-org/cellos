---
title: "Cellos App Tiers Completion"
description: "Risk-gated umbrella program for Native SDK and application-tier completion."
status: in-progress
priority: P1
effort: 37 weeks
branch: main
tags: [app-tiers, sdk, admission, virtualization, abi, security]
created: 2026-08-21
---

# Cellos App Tiers Completion

## Program Contract

This umbrella coordinates child plans for TODO C2–C9. Each child requires approval before Cook. Tier 2 and Manifest v3 require independent security/ABI approvals. Security evidence is non-waivable.

The only successful terminal state is `FULLY_QUALIFIED`: Phase 01's Native SDK contract is approved, its matrix is validated by Phase 02, and every Phase 01–08 child is implemented, verified, and ledger-recorded. Otherwise the program is `NOT_COMPLETE`; BLOCKED/PLANNED work remains visible but cannot close the program or be removed from scope by steering.

## Data Flow

`C2 contract approval + SDK matrix → C8 schema validation → Phase 01–08 evidence → C8 status → C9 closure`. Admission flows `artifact → manifest/signature/provenance → policy/owner decision → execution tier`; Phase 03 remains pending/production-blocked until its child-plan approvals, external-floor qualification, production integration, physical/hostile evidence, authenticated evidence retention under signed CI or a secure measured runner, and ledger closure are complete.

## Phases

| ID | Phase | Effort | Depends on | Status |
|---|---|---:|---|---|
| 01 | [Native SDK Contract C2](phase-01-native-sdk-contract.md) | 2w | — | completed |
| 02 | [Unified Acceptance Ledger C8](phase-02-unified-acceptance-ledger.md) | 1w | 01 | completed |
| 03 | [Tier 1 Baseline/Admission C3](phase-03-tier1-baseline-admission.md) | 4w | 01,02; child Phase 01 approvals; qualified external floor; signed CI or secure measured runner | pending / production-blocked — Core+harness and Phase 04 prequalification infrastructure complete; admissible evidence blocked |
| 04 | [Tier 3 Qualification C5](phase-04-tier3-qualification.md) | 8w | 01,02 | pending |
| 05 | [Manifest-v2 Tooling C7-A](phase-05-manifest-v2-tooling.md) | 2w | 01,02 | completed — v2 compatibility baseline frozen; loader risks transferred |
| 06 | [Tier 1 rust-std PAL C4](phase-06-tier1-rust-std-pal.md) | 3w | 03 | pending — dependency-blocked on Phase 03; feasibility package verified, security backing and human approval blocked |
| 07 | [Tier 2 Native Domain C6](phase-07-tier2-native-domain.md) | 12w | 02,03,04; loader-risk handoff from 05 | pending — dependency-blocked on Phases 03 and 04; owns `CELLOS-LOADER-RACE-002` and `CELLOS-LOADER-CLEANUP-003` |
| 08 | [Manifest-v3 ABI C7-B](phase-08-manifest-v3-abi.md) | 4w | 05,07 | pending — Phase 05 baseline available, still dependency-blocked on Phase 07 |
| 09 | [Final Completion Gate C9](phase-09-final-completion-gate.md) | 1w | 01-08 approved/implemented, verified, ledger-recorded | pending |

## Phase 03 Evidence and Status

The child admission plan records the proposed provenance contract at [`docs/specs/18c-publisher-provenance-envelope.md`](../../docs/specs/18c-publisher-provenance-envelope.md). Its documentation-only Phase 01 recorded host aggregate evidence (`cargo test -p types -p api --target x86_64-unknown-linux-gnu`: 101 passed, 0 failed, 4 ignored), correction of two contract-review defects with a focused recheck passing, and a final independent document review with no blocking document defect.

The authorized Core+harness-only sub-slice is complete. `bash scripts/build-test-hooks-ci.sh` passed exit 0 under RV64 `test-hooks` with `RUSTFLAGS="-D warnings -C relocation-model=pic"`; the bare-metal path logged `[INFO] admission-core self-test PASS (fail-closed A/B floor model)`; the documented QEMU test-hooks runner passed 1/1; all 31 named cases are reachable; no-default and `policy-required,signing-required` RV64 builds passed while excluding hostile/test modules; and the host baseline stayed at 101 passed, 0 failed, and 4 ignored. Independent quality review returned PASS/CORRECT with no findings, and independent static security review returned PASS with zero Critical, High, or Medium findings.

Those reviews are code/evidence review only, not human security-owner approval or independent production-design approval. Umbrella Phase 03 remains **pending / production-blocked**, not complete. Child Phases 02–04 remain **BLOCKED**, and umbrella Phases 06 and 07 remain dependency-blocked. Non-waivable gates remain: both recorded design approvals; a real qualified non-replayable external floor with physical rollback/replay and power-loss evidence; approval of parsers/persistence and provisioning of owner/publisher anchors; controlled final-ELF provenance production; common loader/task-creation runtime wiring and no-task-on-denial proof; final hostile/release approvals; and governed ledger PASS. Changes to the core/harness semantics or evidence surfaces invalidate that sub-slice evidence; changes to anchors, encodings, backend/firmware, recovery, runtime gate, or production profile invalidate the eventual production evidence and approvals.

### Phase 04 Prequalification Infrastructure

The child Phase 04 status is **PREQUALIFICATION INFRASTRUCTURE COMPLETE / ADMISSIBLE EVIDENCE BLOCKED**. Its canonical byte-pinned catalog covers all 18 mandatory rows, maps all 33 stable compiled `C3-ADM-*` test-hooks IDs, and retains a strict ordered runtime parser. The fixed CLI validates only that catalog and has no local capture, runner, custom-input, or evidence-writer path. The former local capture/runner and generated `b7997` bundle were removed because hashing same-process self-reported command, toolchain, source/kernel origin, backend, and replay claims did not authenticate them.

Final local verification passed Python 13/13, RV64 33/33 exactly once in canonical order plus aggregate PASS, documented QEMU integration 1/1, production-build test-marker exclusion, and the unchanged host aggregate of 101 passed, 0 failed, 4 ignored. Final agent quality review returned correct/KEEPABLE with no findings; final agent security review returned PASS with no Critical, High, or Medium findings. Local verification is non-admissible and was not retained. The agent reviews are not human security-owner approval, independent human production-design approval, release approval, or ledger PASS.

Signed CI or a secure measured runner is an explicit prerequisite before retaining content-addressed Phase 04 runtime evidence. A qualified floor with physical replay/power-loss evidence; production parsers, task-creation witnesses and provisioned anchors; controlled final-ELF provenance; the production profile; both required human approvals; release approval; and Phase 02 ledger validation remain open. The ledger is unchanged. Umbrella Phase 03 remains pending/production-blocked, umbrella Phase 04 remains pending, and Phases 06 and 07 remain dependency-blocked.

## Phase 05 Evidence and Status

Umbrella Phase 05 is **completed** at its exact Manifest-v2 tooling boundary. Verification recorded API manifest 8/0, ABI baseline 4/0, host 105/0/4 with four new tests, Python 6/0, direct tool matrix 21/21, a Rust corpus of 128 mutations, a Python corpus of 367 mutation candidates, RV64 `-D warnings` build PASS, a real `spawn_gated` runtime corpus with 12 malformed denials/full scheduler unchanged/one `ELF-LOADER PASS`, documented QEMU integration 1/1, and both production-shaped builds PASS. Independent quality review returned correct with no findings; independent security review found no patch-introduced finding and passed the narrow Phase 05 scope.

The v1-upcast/v2-read corpus, exact v1/v2 lengths and v2 layout, compatibility aliases, default byte-identical v2 writer, canonical protection-class terminology, and tri-state parser contract are frozen as the Phase 08 compatibility baseline. This completion does not enable Tier 2, Manifest v3, or production admission.

Production loader readiness remains blocked by three unresolved pre-existing risks. `CELLOS-LOADER-SIG-001` (Critical) transfers to the Phase 03 provenance/signature boundary because load-affecting section and relocation metadata, including `.rela.dyn`, is not covered and can redirect unchecked kernel writes. `CELLOS-LOADER-RACE-002` (High) transfers to Phase 07 atomic task publication because a task becomes runnable before admission state is installed. `CELLOS-LOADER-CLEANUP-003` (Medium) transfers to Phase 07 denial rollback/cleanup because PlatformCap singleton denial can leave an already-ready task alive. These risks are recorded, not fixed.


## Phase 06 Feasibility Evidence and Status

The Phase 06 child feasibility plan reached **FEASIBILITY PACKAGE VERIFIED / SECURITY BACKING AND HUMAN APPROVAL BLOCKED**. Final verification passed 33/33 feasibility tests, 57/57 validator adversarial attacks, 36/36 security-manifest tamper attacks, and host 105 passed / 0 failed / 4 ignored. Reconciliation confirmed 27/27 modules; 36 hooks at 8 Supported / 10 Unsupported / 18 Deferred; 46 pinned Rust sources; the exact six-path kernel security-backing inventory; and all 101 canonical approval inputs with matching digests and links. Final independent quality review and final independent security review each returned PASS with no findings.

This verified sub-slice is fixture-only and non-promotional. `PAL-019` remains Deferred because the current default tuple enables `dev-weak-rng` over a zero-byte RNG stub; `PAL-031` remains Deferred because `GetRandom` lacks bounded caller-owned writable validation. All six named human approval rows remain `NOT GRANTED`, `PAL-IMPLEMENTATION-CHECKPOINT` remains `BLOCKED`, and no PAL/target/runtime, private or published sysroot/triple, authenticated live capture, readiness, or promotion exists. The recommendation remains **CONDITIONAL GO** only after both security backings are implemented and evidenced, every named human approval is explicitly granted, the checkpoint is unblocked, and umbrella Phase 03 production gates are approved. Umbrella Phase 06 remains pending and dependency-blocked on Phase 03.

## Parallel Ownership

After 01, Phase 02 runs first as the ledger-schema gate. Only after its schema is approved may Phases 03–05 run evidence work in parallel: 03 owns admission and the `CELLOS-LOADER-SIG-001` provenance/signature boundary, 04 hypervisor lanes, and completed 05 owns the frozen v2 tooling/tri-state baseline. Phase 07 receives loader/task ownership after Phase 05 verification and must resolve `CELLOS-LOADER-RACE-002` plus `CELLOS-LOADER-CLEANUP-003`; it remains blocked on Phases 03 and 04. Phase 08 receives manifest ABI ownership after 05/07 and remains blocked on Phase 07.

## Program Gates

No Tier 2 exposure before Spec 22 negatives and resolution of Phase 07 loader/task atomicity and cleanup risks. No Manifest v3 without completed Phase 07 qualification and 2× ABI approval. The Raspberry Pi 3 ARM64 lane must be hardware-qualified. PASS needs reproducible evidence. V2, Tier 1 SAS fast path, and Tier 3 fallback remain compatible. Phase 01 must be contract-approved and its SDK matrix accepted by Phase 02; every implementation child then follows `design-approved → child-linked → implemented → verified → ledger-recorded`. Skipping a state or remaining BLOCKED/PLANNED prevents closure. Phase 05 completion alone grants no Tier 2, v3, or production-admission claim.

## Approval Matrix

| Gate | Approver roles | Required artifact | Independence | Invalidation |
|---|---|---|---|---|
| SDK/baseline | SDK owner + tester | contract, conformance report | tester is not implementer | public/API drift |
| Tier 1 admission | security owner + reviewer | threat model, negative evidence | reviewer did not author change | key/policy/provenance change |
| Tier 3 lane | platform custodian + tester | hardware log, recovery report | evidence runner differs from implementer | firmware/hardware/VMM change |
| Tier 2 | security owner + two reviewers | Spec 22 matrix, rollback drill | reviewers independent of implementer | MMU/syscall/grant/DMA change |
| Manifest v3 | ABI owner + two explicit confirmations | ADR, fixture hashes, migration report | confirmations from distinct roles | layout/parser/signing change |

## Rollback

Rollback is child-scoped. Tier 2 drains to safe roots and reboots; never converts domains to SAS. V3 rollback keeps v2 writer/parser. Published stable SDK/ABI promises cannot silently disappear.

## Validation

Full verification roles; re-grep cited paths and enumerate changed-contract consumers before each child approval.

## Red Team Review

Adjudicated findings added one fully-qualified terminal state, mandatory child lifecycle, independent approvals, content-addressed evidence, anti-rollback admission, hardware custody, hostile/failure tests, bounded Tier 2 rollback, and authenticated v3 downgrade protection. No evidence may be waived and steering cannot remove C2–C9 scope.

## Validation Log

1. **1B:** only `FULLY_QUALIFIED|NOT_COMPLETE`; impacts Phase 02 and Phase 09 closure.
2. **2A override:** primary Tier 3 hardware is available ARM64 Raspberry Pi 3; impacts Phase 04 evidence gate.
3. **3A:** signed atomic A/B owner store with monotonic generation/fail-closed recovery; impacts Phase 03.
4. **4A:** v3 extends publisher-signed envelope; owner consent stays separate; impacts Phase 08.
5. **5A:** rust-std p99 syscall/IPC regression ≤5% under pinned measurement rules; impacts Phase 06.
