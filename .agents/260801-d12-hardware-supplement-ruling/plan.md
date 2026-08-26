# D12 hardware supplement ruling

**Status:** Complete  
**Decision:** Recommendation A approved 2026-08-01  
**Evidence:** `../reports/d12-hardware-supplement-set-analysis-260801.md`

## Scope

- Make Spec 19 the sole owner of the Layer A/B/C hardware-isolation taxonomy.
- Correct stale MTE/MPK/PMP and speculative-side-channel claims.
- Correct PKU enforcement and self-test status without changing runtime code.
- Preserve concurrent working-tree changes and public/runtime contracts.

## Changes

- [x] Replace the Spec 05 supplement table with a Spec 19 pointer and status summary.
- [x] Rewrite Spec 16 §3.3; retain Spec 12's PMP statement with a cross-reference.
- [x] Correct directly implicated testing, kernel-boundary, roadmap, architecture,
      security-model, Spec 19, and changelog claims.
- [x] Mark D12 RULED/APPLIED in its analysis report and decision docket.
- [x] Validate cross-references, stale-claim searches, Markdown whitespace, and index state.

## Evidence

- Stale-claim assertions: PASS.
- Spec 19 and Spec 12 link targets: present.
- `git diff --check`: PASS.
- Git index: empty.
- Runtime tests: not run; D12 changes documentation only.
- Planner/tester/reviewer/project-manager/docs-writer agents were attempted but unavailable
  because the configured provider returned `404 No active credentials`; local fallback
  planning, validation, and review completed the same gates.

## Success criteria

- No document calls MTE or MPK a Spectre/Meltdown mitigation.
- No document claims current PKU PTE enforcement or a denied-access PKU self-test.
- PMP remains described as unavailable to the S-mode runtime without an M-mode owner.
- Specs 05/12/16 refer to Spec 19 instead of duplicating a competing taxonomy.
- No runtime code or ABI changes.
