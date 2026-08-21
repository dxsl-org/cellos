# Phase 03 — Tier 1 Baseline and Admission C3

## Context Links
`.agents/TODO.md:14-19`; `docs/specs/18-cell-trust-tiers.md:84-129`; `docs/specs/18b-cell-admission-consent-adr.md:9-24`; `docs/specs/18c-publisher-provenance-envelope.md`; `docs/roadmap/open-risk-register.md:26-30`.

## Overview
Lock `rust-no-std` baseline and truthful production admission. **Status: pending (production-blocked); Core+harness-only and Phase 04 prequalification-infrastructure sub-slices: complete.** The documentation-only publisher-provenance design is ready for required approval. Neither that design nor either completed bounded sub-slice qualifies a floor, creates admissible runtime evidence, completes this umbrella phase, enables production admission, or permits ledger PASS.

## Key Insights
Publisher provenance and owner authorization are independent; developer mode is not production.

## Documentation Evidence

- Child plan: `.agents/260821-1700-tier1-baseline-admission/`; proposed contract: `docs/specs/18c-publisher-provenance-envelope.md`.
- Host aggregate baseline command `cargo test -p types -p api --target x86_64-unknown-linux-gnu` completed with 101 passed, 0 failed, and 4 ignored.
- Contract review found two specification defects; corrections passed focused recheck. A final independent document review found no blocking document defect.
- This review evidence is not the required security-owner approval or the required approval by an independent reviewer who did not author the design.

## Core/Harness Completion Evidence

- `bash scripts/build-test-hooks-ci.sh` passed exit 0, compiling the RV64 `test-hooks` kernel with `RUSTFLAGS="-D warnings -C relocation-model=pic"`.
- The bare-metal path logged `[INFO] admission-core self-test PASS (fail-closed A/B floor model)`; the documented QEMU test-hooks runner `cd tests/integration && CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu cargo test --test vfs-quota` passed 1/1; all 31 named admission cases are reachable.
- RV64 no-default and `policy-required,signing-required` builds both passed while excluding the hostile/test modules. The host baseline remained 101 passed, 0 failed, and 4 ignored.
- Independent quality review returned PASS/CORRECT with no findings. Independent static security review returned PASS with zero Critical, High, or Medium findings.

This completes only the backend-neutral decision core and explicitly non-qualifying `test-hooks` harness. The reviews are code/evidence review, not human security-owner approval or independent production-design approval. Open non-waivable blockers remain: a qualified non-replayable floor and physical replay/power-loss evidence; approved parsers and persistence; boot provisioning of owner/publisher anchors; controlled provenance production; common loader/task-creation runtime wiring and no-task-on-denial proof; human approvals; Phase 04 hostile/release evidence; and governed ledger PASS. Exact evidence and invalidation rules are retained in `.agents/260821-1700-tier1-baseline-admission/phase-03-core-harness-blocker-resolution.md`.

## Phase 04 Prequalification Infrastructure Evidence

- **PREQUALIFICATION INFRASTRUCTURE COMPLETE / ADMISSIBLE EVIDENCE BLOCKED:** the byte-pinned catalog covers all 18 mandatory Phase 04 matrix rows, maps all 33 stable compiled `C3-ADM-*` test-hooks IDs, and retains a strict ordered runtime parser as pure validation logic. The CLI validates only the canonical catalog and has no capture, runner, custom-input, or evidence-writer path.
- Final local verification passed Python 13/13; RV64 all 33 IDs exactly once in canonical order plus aggregate PASS; documented QEMU integration 1/1; both production-shaped builds with test-marker exclusion; and the unchanged host aggregate of 101 passed, 0 failed, 4 ignored. These local results are non-admissible, and no runtime log or evidence artifact was retained.
- Final agent quality review returned correct/KEEPABLE with no findings; final agent security review returned PASS with no Critical, High, or Medium findings. Neither review is human security-owner approval, independent human production-design approval, release approval, or ledger PASS.
- The former local capture/runner and generated `b7997` bundle were removed rather than relabeled. Their same-process self-reported environment, kernel/source origin, backend, and replay claims were not authentic evidence.
- Signed CI or a secure measured runner is now an explicit prerequisite before any content-addressed Phase 04 runtime evidence is retained. It must authenticate execution provenance and actual replay/fault evidence. The qualified floor, production parser/task wiring and anchors, controlled final-ELF provenance, production profile, human approvals, release approval, and Phase 02 ledger validation remain blocked.

## Requirements
Characterize SDK/examples/FFI/Lua and bind source/dependencies/toolchain/recipe/ELF digest into publisher provenance. The authoritative owner-admission design is a signed, digest-pinned atomic A/B store whose generations are bound to a non-replayable floor outside both replaceable slots: a qualified hardware monotonic counter or authenticated append-only anchor with equivalent rollback resistance. Each slot carries authenticated contents, generation, transaction identity, and commit marker. Production admission stays disabled until the floor backend qualifies. Test wrong-key/tamper/old-generation/torn-write/power-loss plus replay of both otherwise-valid old slots.

## Architecture
Publisher evidence → owner-signed record → write/verify inactive slot at `floor+1` with transaction intent → atomically advance external floor under that intent → finalize matching slot commit → admit only a fully authenticated slot whose generation equals the floor. If floor exceeds both slots, fail closed into recovery. If any slot exceeds floor, treat it as an incomplete transaction and run the explicit atomic recovery protocol; never auto-admit or advance from slot contents alone.

## Assumptions
Floor backend choice is `[UNVERIFIED]`; production remains disabled until its non-replay, atomicity, durability, and recovery behavior qualify. Replaceable A/B media is never itself the floor.

## Related Code Files
`kernel/src/signing.rs:21-64`; `kernel/src/policy.rs:85-170`; `kernel/src/loader.rs:115-192`; `kernel/src/loader/mem_spawn_gate.rs:30-64`; `kernel/src/main.rs:871-877`; `scripts/sign-cell.py:61-317`.

## Implementation Steps
Freeze baseline; qualify external floor backend; design source-to-ELF provenance and signed A/B transaction binding; specify floor/slot mismatch recovery; enumerate callers; implement in child plan; inject replay-both-slots, torn-write, floor-ahead, slot-ahead, and power-loss failures; update ledger.

## Todo List
- [x] Publish the documentation-only baseline/provenance contract and record its test/review evidence.
- [x] Complete and independently review the backend-neutral core plus non-qualifying `test-hooks` harness.
- [ ] Security owner approves the provenance/owner separation, exact encoding, A/B recovery state machine, and threat model.
- [ ] Independent reviewer who did not author the design approves it separately.
- [ ] Qualify an actual external non-replayable floor with authenticated intent binding and real rollback/replay and power-loss evidence.
- [ ] Implement boot-provisioned anchors and mandatory publisher-plus-owner production admission, then pass hostile evidence and ledger closure.
- [ ] Keep production/developer claims separated.

## Success Criteria
Production cannot enable without a qualified external floor. Governed paths admit only `publisher ∧ owner slot generation == floor`; replaying both old valid slots, floor-ahead, slot-ahead, partial writes, or invalid transaction binding creates no task/fallback. Recovery never auto-admits or derives the floor from slots.

## Risk Assessment
Lockout, floor loss, rollback replay, or accidental SAS admission. Any unresolved floor/slot mismatch enters authenticated recovery or denies; it never selects “highest slot.” Developer posture is a separate explicit reboot choice; floor reset is a separately authorized reprovisioning event.

## Security Considerations
Keys are provisioned/rotatable; authority only narrows; malformed stores fail closed.

## Next Steps
Obtain the two required Phase 01 human approvals; qualify an actual external floor with physical replay/power-loss evidence; provision signed CI or a secure measured runner before retaining admissible Phase 04 runtime evidence; and complete the blocked production integration, hostile/release evidence, and ledger gates. Umbrella Phase 03 remains pending/production-blocked, umbrella Phase 04 remains pending, and Phases 06 and 07 remain dependency-blocked until their complete prerequisites—not merely the bounded Core/harness and prequalification-infrastructure sub-slices—are implemented, verified, human-approved where required, and ledger-recorded.

## Deviation Log
- 2026-08-21: Recorded the completed Core+harness-only sub-slice and its final build, execution, reachability, baseline, and independent agent-review evidence. Umbrella Phase 03 remains pending/production-blocked; all floor, integration, human-approval, physical-evidence, authenticated-runner, and ledger gates remain open.
- 2026-08-21: Recorded Phase 04 prequalification infrastructure as complete and admissible evidence as blocked. The verified 18-row/33-ID catalog/parser has no local evidence writer; the former local capture/bundle was removed. Signed CI or a secure measured runner is required before retention, the ledger is unchanged, and no agent review is treated as human approval.
