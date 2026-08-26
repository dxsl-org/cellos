---
title: "Phase 04 - Negative Evidence, Approval, and Ledger Closure"
status: blocked
priority: P1
effort: 3d
depends_on: ["phase-03-provisioned-admission-gate"]
owner: "security verification"
---

# Phase 04 - Negative Evidence, Approval, and Ledger Closure

## Context Links

- Umbrella success/risk/security gates: `.agents/260821-0642-app-tiers-completion/phase-03-tier1-baseline-admission.md:33-40`
- Approval matrix: `.agents/260821-0642-app-tiers-completion/plan.md:42-54`
- Phase 02 ledger lifecycle: `.agents/260821-0642-app-tiers-completion/phase-02-unified-acceptance-ledger.md:15-25,40-44`
- Existing signer tests: `scripts/test_cellos_sign.py:255-349`; `scripts/test-cell-signing.sh`
- Existing loader tests: `kernel/src/loader/elf_tests.rs:16-54,567-631`

## Overview

Produce hostile, reproducible evidence that publisher provenance plus owner authorization is enforced before task creation and cannot be rolled back through its local A/B media. Reviewers assess the evidence, not merely implementation intent. No production feature/profile is enabled and no ledger PASS is recorded before all scenarios pass and the independent approvals are present.

## Key Insights

- Positive signatures and nominal A/B writes do not demonstrate rollback resistance. The decisive attack restores both locally valid old slots after the external floor has advanced.
- The old loader's absent-signature dev behavior must not leak into the production profile. A production build that silently falls back to default features is a security regression even if happy-path signed cells run.
- Test doubles are valuable for exhaustive state injection but do not qualify a floor backend; the actual candidate must undergo the same replay/power-loss drills.

## Requirements

- Use focused unit/state-machine tests for parsers and all floor/slot transitions; use target/image/runtime evidence to show denied cases create no task, no scheduled fallback, and correct non-secret audit event.
- Test every spawn source that reaches the common gate: boot/path bytes and caller-supplied `SpawnFromMem` bytes with hostile names. Separately audit direct TCB boot spawns to prove their exemption is non-generalizable.
- Retain content-addressed test logs, image/ELF/provenance/store/floor fixture digests, build revision and dirty-state digest, command/toolchain/environment, backend identity/firmware, timing of failure injection, owner, and expiration/TTL only after a signed CI job or secure measured runner can authenticate those inputs and replay resistance. Local execution is verification only, is explicitly non-admissible, and must not be retained as Phase 04 evidence.
- Rerun the approved negative suite after any change to keys/anchor provisioning, producer/provenance encoding, signing payload rules, owner-store schema, external-floor backend/firmware, recovery logic, common loader gate, or production feature profile.

## Final Verified/Reviewed Prequalification State (Non-Admissible)

- **PREQUALIFICATION INFRASTRUCTURE COMPLETE:** `scripts/admission_prequalification/catalog.json` is the byte-pinned machine-readable inventory for all 18 mandatory matrix rows. It maps all 33 stable compiled `C3-ADM-*` test-hooks IDs bidirectionally and leaves parser, task-creation, production-profile, real-backend, physical-fault, and approval-dependent rows explicitly `BLOCKED`.
- `scripts/validate-admission-prequalification.py` authoritatively validates only that canonical catalog. It accepts no capture, log, context, kernel, backend, catalog, source, or output arguments and emits no manifest, bundle, log, or other evidence artifact.
- `scripts/admission_prequalification/validator.py` and `tests/admission-prequalification/test_prequalification.py` retain the canonical 18-row semantic/mapping pin, all 33 stable IDs, and the strict ordered runtime-log parser as pure validation logic.
- `C3-ADM-001/002` cover the valid normal state in which an authentic stale partner cannot displace the externally-current slot. `C3-ADM-032/033` separately cover replay of old A/B when the externally-current partner is missing or invalid, requiring recovery rather than fallback to the old slot.
- Final local verification passed: the focused Python suite passed 13/13; the RV64 test-hooks run observed all 33 IDs exactly once in canonical order plus the single aggregate PASS; the documented QEMU integration passed 1/1; both production-shaped RV64 builds passed and excluded the `C3-ADM-`, self-test PASS, and self-test FAIL markers; and the host aggregate remained 101 passed, 0 failed, 4 ignored with zero baseline delta. No runtime log or evidence artifact was retained.
- Final quality review returned correct/KEEPABLE with no findings. Final security review returned PASS with no Critical, High, or Medium findings. These are agent code/evidence reviews only; neither is human security-owner approval, independent human production-design approval, release approval, or ledger PASS.
- The former P04-PREQ-002 local capture/runner path and generated `b7997` bundle were safely removed rather than accepted or relabeled. The removed design could only make same-process, self-reported claims about its command, toolchain, source, kernel origin, backend, and replay resistance; hashing those claims did not authenticate them.
- **ADMISSIBLE EVIDENCE BLOCKED:** signed CI or a secure measured runner is now an explicit prerequisite before retaining any content-addressed Phase 04 runtime evidence. It must authenticate its shell, toolchain, final/prebuilt-kernel origin, source state, backend identity, physical or equivalent non-replayable fault execution, and replay resistance; a local process cannot establish those claims about itself.
- Production parsers and task creation, provisioned publisher/owner anchors, controlled final-ELF provenance, a qualified external floor, physical replay/power-loss evidence, the named production profile, both required human approvals, release approval, and Phase 02 ledger validation remain gated. Production remains disabled; the ledger is unchanged; this phase remains **BLOCKED**.

## Mandatory Negative Scenario Matrix

| Scenario | Required result |
|---|---|
| unsigned ELF; stripped `__ViCell_sig`; malformed/missing/short signature | production gate denies before parse/task creation; no dev fallback |
| wrong publisher key, dev key, unchecked-dev signature, tampered PT_LOAD, tampered manifest | Claim A denies; owner entry cannot override |
| valid current payload signature but missing/malformed/unknown-version provenance envelope | Claim A denies |
| final ELF hash mismatch; stale source/lock/toolchain/recipe identity; envelope from another ELF | Claim A denies |
| missing owner anchor, wrong owner key, unsigned store, malformed store, unknown store version, duplicate/conflicting owner records | Claim B denies without panic |
| valid publisher provenance but no owner entry; owner entry with stale/wrong ELF or provenance digest | Claim B denies |
| owner record attempts to authorize unsigned/wrong-key/tampered/unprovenanced ELF | publisher failure still denies (narrow-never-widen invariant) |
| authentic stale partner beside one externally-current slot; wrong expected generation; wrong transaction ID/intent digest; uncommitted, missing, or invalid slot | current slot is never displaced in the valid stale-partner state; all hostile/invalid states recover or deny with no task or slot-derived floor advance |
| torn slot write or power loss before write, after intent write, after slot verification, after external advance, and before/after commit marker | deterministic authenticated recovery/deny only; never auto-admit |
| floor is ahead of both slots | deny/recovery-required; never choose highest slot or reconstruct/advance floor from slots |
| either slot is ahead of floor | deny/recovery-required; never admit/advance from slot contents |
| **replay slot A with a previously valid old generation, with current B present and unavailable** | valid current B wins without displacement; without valid current B, recover/deny with no old-A fallback |
| **replay slot B with a previously valid old generation, with current A present and unavailable** | valid current A wins without displacement; without valid current A, recover/deny with no old-B fallback |
| **replay both otherwise-valid old A and B slots together** | deny after floor advanced; no rollback, no task, no auto-repair that derives floor from slots |
| replay old external read/advance response; conflicting same-generation intent; duplicate advance | authenticated floor contract rejects/identifies it; no ambiguous admission |
| floor unavailable, backend identity changed/replaced, counter exhausted, unauthorized reset | deny; only separately authorized reprovisioning may restore service |
| `SpawnFromMem` with `/bin/...`, traversal, separator, or long hostile name | common publisher/owner gate still applies; label cannot select owner entry or path privilege |
| invalid policy/path/manifest after valid admission | existing later narrowing still denies where appropriate; admission does not widen capabilities |

## Related Code Files

- `kernel/src/admission.rs` (planned)
- `kernel/src/signing.rs`, `kernel/src/loader.rs`, `kernel/src/loader/mem_spawn_gate.rs`, `kernel/src/main.rs`, `kernel/src/audit.rs`, `kernel/src/measurement_log.rs`
- `kernel/src/loader/elf_tests.rs` and new focused admission/floor tests
- `scripts/test_cellos_sign.py`, `scripts/test-cell-signing.sh`, controlled image-lane tests
- qualified backend test harness and platform evidence records
- `docs/app-tier-acceptance-ledger.json` and its Phase 02 validator/evidence projection

## Implementation Steps

1. Convert the matrix into named deterministic tests that assert returned error, task-count/scheduler state, audit reason, and floor/slot post-state for each branch.
2. Build replayable slot/floor fixtures from valid signed records so replay tests prove anti-rollback rather than merely malformed-input rejection.
3. Inject loss/corruption at each write/verify/advance/commit boundary, reboot/reload the real state, and assert only approved recovery/denial behavior.
4. Execute actual-backend qualification drills, including physical or equivalent non-replayable rollback/power-failure evidence. Keep fakes as exhaustive adjuncts, not as qualification substitutes.
5. Under signed CI or a secure measured runner, capture production-profile provenance pipeline evidence and verify that dev features/keys/weak RNG/fallback paths are absent.
6. Security owner reviews the threat model and evidence; an independent reviewer who did not author or implement the change reviews code and evidence separately. Resolve findings and collect explicit PASS artifacts.
7. Submit the immutable evidence set to Phase 02. Its validator records PASS only when every required witness is valid; otherwise record BLOCKED/FAIL and keep production disabled.

## Todo List

- [ ] Every mandatory negative case has a named test and retained result.
- [ ] Each individually replayed old slot is exercised with its externally-current partner valid, missing, and invalid; the combined replay is also exercised after a real floor advance.
- [ ] Every crash boundary is injection-tested against the qualified backend.
- [ ] No denial creates a task, chooses a fallback slot, or advances a floor from local storage.
- [ ] Security owner approval artifact recorded.
- [ ] Independent reviewer approval artifact recorded.
- [ ] Phase 02 ledger validates content-addressed PASS evidence.

## Acceptance Criteria

- No content-addressed Phase 04 evidence is retained until signed CI or a secure measured runner authenticates execution provenance and replay resistance.
- The negative matrix passes for the named production configuration and real qualified floor backend.
- Replaying both valid old slots cannot produce admission even if both owner signatures and stored records verify locally.
- Replaying either valid old slot without a valid externally-current partner requires recovery and cannot select the old slot as a fallback.
- `publisher ∧ owner slot generation == external floor` is the sole production admission condition; any ambiguity fails closed.
- Security-owner and independent-reviewer approval artifacts are both present and valid for the exact source/evidence digests.
- Without qualified external-floor evidence, Phase 02 records BLOCKED and production admission remains disabled.

## Risk Assessment

- **False confidence from happy-path evidence:** mitigated by mandatory wrong-key, provenance, stale-digest, rollback, torn-write, and crash injection cases.
- **Evidence/backend drift:** mitigated by backend/firmware identity capture and invalidation rules.
- **Review independence failure:** mitigated by explicit role/author separation in approval artifacts and Phase 02 validation.

## Security Considerations

A test that merely restores one invalid blob is insufficient: the matrix requires attacker-quality replay of authentic historical records, especially both old slots. Recovery code is part of the attack surface and must demonstrate denial rather than availability bias. Audit output must not leak key or confidential provenance material.

## Rollback

A failed negative scenario, expired evidence, rejected review, or backend change immediately demotes the ledger state and disables the production profile. It does not roll the floor backward or authorize old slots. Restore service only through the separately approved reprovisioning procedure.

## Next Steps

On PASS, Phase 02 records the Phase 03 lifecycle evidence. Umbrella Phase 03 then unblocks Phase 06 and transfers loader ownership to Phase 07 only under its documented dependency rules.

## Deviation Log

- 2026-08-21: Completed and independently reviewed the bounded 18-row/33-ID prequalification inventory, catalog validator, and strict runtime parser. Removed the non-admissible local capture/writer and its generated bundle. Local verification passed but was not retained as evidence; signed CI or a secure measured runner is required before admissible Phase 04 evidence retention. Phase 04 remains BLOCKED and the ledger is unchanged.
