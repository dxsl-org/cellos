---
phase: 5
title: "Decision Package and Approval Checkpoint"
status: "FEASIBILITY PACKAGE VERIFIED / SECURITY BACKING AND HUMAN APPROVAL BLOCKED"
priority: P1
effort: 0.5d
dependencies: [2, 3, 4]
tier: thinking
---

# Phase 05: Decision Package and Approval Checkpoint

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log when it occurs. Approval absence, scope expansion, or blocker removal is contract-breaking and must be escalated, never inferred.

## Overview

Assemble the feasibility artifacts and fixture-only validator into one canonical approval-input package, record independent package verification, preserve every absent human approval, and stop at the later PAL/target/runtime implementation checkpoint. This phase verifies only the feasibility package; it does not imply that security backing is complete, any approval exists, or a PAL, runtime, target, live benchmark capture, or promotion exists.

## Requirements

- Reconcile source/module/hook completeness, the exact kernel security-backing inventory, compiler choice, runtime/API contract, workload parity, implemented validator/schema/tests/fixtures/reports, owners, risks, and rejection criteria.
- Present terminal state is exactly **FEASIBILITY PACKAGE VERIFIED / SECURITY BACKING AND HUMAN APPROVAL BLOCKED**. The recommendation is **CONDITIONAL GO** only after `PAL-019`, `PAL-031`, and every other blocker are implemented and evidenced. Verification authorizes neither implementation nor approval; `NO_GO` remains the fail-closed alternative if any input or blocker fails.
- One canonical `artifacts/approval-input-manifest.json` must content-address every plan, contract, transitive pinned/Cellos source, every exact kernel security-backing path, tool, test, fixture, and expected report. Approval and checkpoint records bind its digest.
- No approval may be inferred from review prose, missing signatures, planned work, local measurements, steering, package verification, or a conditional recommendation.

## Architecture

`artifacts/approval-input-manifest.json` is the canonical digest index for every approval input. It includes roles and SHA-256 for all plans/contracts, the hook/source map plus every transitive pinned Rust and Cellos backing source, the exact closed kernel security-backing path set, validator/schema/CLI, both tests, every fixture, and both expected reports. The manifest itself, the decision package, and approval/checkpoint records are explicitly excluded from its input list to avoid a hash cycle; those records embed the manifest digest. `artifacts/implementation-decision-package.md` records totals, blocking Deferred rows, selected/rejected compiler strategies, contracts, validator inventory, risks, approvals, blockers, invalidation triggers, and explicit non-claims.

## Assumptions

None — only content-addressed inputs enter the approval-input manifest. Verification is complete; security backing and all six named human approvals remain separate blocked gates.

## Related Files

- Read only: `artifacts/pal-hook-support-map.json`, `artifacts/compiler-strategy-decision.md`, `artifacts/runtime-api-contract.md`, `artifacts/workload-parity-spec.md`, `artifacts/benchmark-validator-contract.md`
- Read only: `scripts/rust_std_promotion/{__init__.py,validator.py,schema_validation.py,benchmark-run.schema.json}`, `scripts/validate-rust-std-promotion.py`, both tests, every Phase 04 fixture, and both expected reports
- Read only: every pinned Rust and Cellos source named transitively by the hook/source map and all six paths in its exact kernel security-backing inventory
- Records excluded from the approval-input hash cycle: `artifacts/implementation-decision-package.md`, `approvals/*.md`
- Create: `artifacts/approval-input-manifest.json`, `artifacts/implementation-decision-package.md`, `approvals/implementation-checkpoint.md`

## Implementation Steps

1. Build the canonical approval-input manifest over every prerequisite plan/contract/source/tool/test/fixture/report, including exact equality with the hook map's closed kernel security-backing path set; verify each path/role/digest mechanically during the later verification stage.
2. Keep the decision package and approval/checkpoint records outside the input set, document the exclusion, and bind each record to the same approval-input-manifest digest.
3. Reconcile module/hook/API statuses with the compiler choice. `PAL-019` and `PAL-031` remain blocking Deferred. Any omitted module/security path, blocking Deferred hook, unsupported selector, unowned toolchain fork, hidden POSIX/ambient authority, production `dev-weak-rng`, missing bounded writable validation or hostile evidence, or unapproved frozen-ABI drift forces `NO_GO`.
4. Confirm the validator remains fixture-only, non-promotional, physical-order preserving, whole-cohort fail-closed on interference, and closed over linker inputs.
5. Record approvals from the six named roles without changing `NOT GRANTED` until each named human signs the same independently verified manifest digest after all security-backing blockers are evidenced.
6. Permit a later PAL/target/runtime implementation child only after every contract approval, the implementation checkpoint, and umbrella Phase 03 production gates are explicitly approved.

## Non-Waivable Blockers

- Umbrella Phase 03's design, external-floor, provenance, production-integration, hostile/physical evidence, authenticated-retention, release, and ledger gates remain open until explicitly approved.
- Compiler integration must select the internal PAL under pinned source with reproducible provenance; unsupported fallback, external plug-in claims, and fake `std` are prohibited.
- Current entropy backing is non-qualifying: the production evidence tuple must omit `dev-weak-rng` and prove real entropy or a zero/error result with no synthetic success.
- `GetRandom` technical backing/evidence is complete: bounded caller-owned writable validation and null/overflow/oversized/unmapped/kernel/peer hostile direct-opcode evidence are retained, but `PAL-031` remains Deferred pending named approval of this governed rebind.
- The six-path kernel security-backing inventory is closed; path-set or digest drift invalidates the package and cannot be omitted from future approval inputs.
- Hook-map, runtime/API, workload-parity, and benchmark-validator approvals must all be explicit and current.
- Any frozen-ABI change needs repository-mandated 2× explicit confirmation before implementation planning.
- A published target/triple, mlibc, PAL/target/runtime source, live benchmark capture, or promotion claim cannot bypass the checkpoint.

## Success Criteria

- [x] Independent test and review set the package only to **FEASIBILITY PACKAGE VERIFIED / SECURITY BACKING AND HUMAN APPROVAL BLOCKED**; **CONDITIONAL GO** remains contingent and grants no approval.
- [x] One canonical approval-input manifest covers every plan/contract, transitive source, the exact six-path kernel security-backing inventory, tool, both tests, every fixture, and both expected reports without a self-reference cycle.
- [x] Approver roles, independence, all six `NOT GRANTED` decisions, `PAL-019`/`PAL-031` blockers, non-claims, and invalidation triggers are explicit and bind the same manifest digest.
- [x] Later PAL/target/runtime implementation remains blocked until the entropy prerequisite, every remaining Deferred prerequisite, all named approvals, the checkpoint, and umbrella Phase 03 production gates are approved.
- [x] Umbrella Phase 06 remains pending; this child makes no PAL/target/runtime or promotion-completion claim.

## Verification and Review Evidence

Final verification passed 33/33 feasibility tests, 57/57 validator adversarial attacks, 36/36 security-manifest tamper attacks, and the host aggregate of 105 passed, 0 failed, and 4 ignored. Reconciliation confirmed 27/27 modules; 36 hooks at 8 Supported / 10 Unsupported / 18 Deferred; 46 pinned Rust sources; the exact six-path kernel security inventory; and all 106 approval inputs with matching digests and links. Final independent quality review returned PASS with no findings, and final independent security review returned PASS with no findings. All six named human approval rows remain `NOT GRANTED`; `PAL-IMPLEMENTATION-CHECKPOINT` remains `BLOCKED`.

## Security Considerations

Approval records bind role, decision, canonical approval-input-manifest digest, date, and independence. The manifest closes future approval inputs without hashing the records that embed its digest. The fixture-only validator rejects provenance substitution, order repair, selective sample rejection, and linker-input substitution, but it creates no authenticated evidence and cannot replace Phase 03 security approval.

## Risk Notes

An implemented or independently verified feasibility package could be mistaken for PAL implementation or approval. Exact two-stage vocabulary, `NOT GRANTED` records bound to the canonical manifest, fixture-only reports, non-claims, and dual checkpoint conditions prevent that state collapse.

## Deviation Log

None.
