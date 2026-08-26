# Phase 05 — Manifest-v2 Tooling C7-A

## Context Links
`.agents/TODO.md:48-54`; `docs/specs/05-application.md:29-31`; `libs/api/src/abi/manifest_flags.rs:10-50`; `libs/api/src/abi/manifest_parse.rs:32-76`.

## Overview
Normalize developer terminology while preserving Manifest v2 layout and aliases. Implementation, verification, and independent review are complete.

## Key Insights
V2 `tier` is protection class, not execution tier.

## Requirements
Display execution tier/profile/protection/capabilities/evidence separately. Preserve v1 upcast, v2 layout, `tier`, and `TIER_*`; no ABI/layout change. Parser/loader contract is tri-state: `Absent` follows the existing explicit no-manifest policy, `Valid(v1|v2)` continues validation, and `Malformed` is denied before task creation. Reject fictional Tier 2. Test v1/v2 × aliases × flags × round-trip plus absent, truncated, oversized, unknown-version/flag, duplicate-section, boundary-value, and mutation/fuzz corpus. Require byte-identical compatibility and panic-free fail-closed parsing.

## Architecture
ELF manifest section → `Absent | Valid(v1|v2) | Malformed` → governed `spawn_gated` decision before task publication → canonical labels/tool display; writer remains byte-identical v2.

## Assumptions
Execution-tier selection remains external policy; no persistent fields.

## Related Code Files
Owned child-plan surfaces: `libs/api/src/abi/manifest.rs:47`; `libs/api/src/abi/manifest_flags.rs:7-95`; `libs/api/src/abi/manifest_parse.rs:9-76`; `libs/api/src/abi/manifest_tests.rs:1-35`; `kernel/src/loader.rs:115-192`; `kernel/src/loader/mem_spawn_gate.rs:30-64`; `kernel/src/loader/elf_tests.rs:331-493`; `tools/check_elf.py`. Loader ownership transfers to Phase 07 only after Phase 05 verification.

## Implementation Steps
Enumerate consumers; specify tri-state without layout change; add labels; preserve fixtures; build malformed/fuzz corpus; prove `Malformed` denial through governed `spawn_gated` tests before task creation; verify `Absent` policy separately; update ledger.

## Implementation Status
**Completed.** The ABI parser requires exact v1/v2 lengths; the loader has a bounded tri-state classifier and denies malformed records before signature parsing or task creation; the inspector and focused Rust/Python behavioral corpora are present.

Verification recorded: API manifest 8 passed/0 failed; ABI baseline 4/0; host aggregate 105 passed/0 failed/4 ignored, including four new tests; Python suite 6/0; direct tool matrix 21/21; Rust mutation corpus exercised 128 mutations; Python mutation corpus exercised 367 candidates; RV64 `-D warnings` build PASS; real `spawn_gated` runtime corpus denied all 12 malformed inputs with the full scheduler unchanged and emitted one `ELF-LOADER PASS`; documented QEMU integration 1/1; and both production-shaped builds PASS. Independent quality review returned correct with no findings. Independent security review found no patch-introduced findings and passed the narrow Phase 05 boundary.

## Frozen V2 Compatibility Baseline
Phase 05 freezes the accepted v1-upcast/v2-read corpus, exact v1/v2 lengths and v2 layout, compatibility names (`tier`, `TIER_*`, and existing manifest macro forms), default byte-identical v2 writing, canonical protection-class labels, and `Absent | Valid(v1|v2) | Malformed` classification. Phase 08 must preserve this baseline, pin its fixture-corpus hash before any v3 ABI work, and keep v2 as the default writer until its separate approvals complete.

## Todo List
- [x] Consumers inventoried.
- [x] V2 bytes unchanged.
- [x] Tests pass; no Tier 2 UI.

## Success Criteria
All valid/absent fixtures retain intended semantics; v2 bytes/layout unchanged; every malformed/fuzz case is denied before task creation through `spawn_gated` without panic/over-read; tools never call protection class an app tier.

## Risk Assessment
Phase 05 did not change the Manifest v2 ABI. Reverting labels remains possible while retaining the fail-closed parser; emitted v2 remains valid.

## Security Considerations
Unknown flags/versions fail closed; capabilities/evidence display honestly. Narrow Phase 05 acceptance does **not** establish Tier 2, v3, or production admission readiness. Three adjacent pre-existing loader findings remain unresolved and production-blocking: `CELLOS-LOADER-SIG-001` (Critical: unsigned load-affecting section/relocation metadata can redirect `.rela.dyn` into unchecked kernel writes; Phase 03 provenance/signature-boundary owner), `CELLOS-LOADER-RACE-002` (High: a task becomes ready/runnable before allowlist, quota, policy, capabilities, and protection state are installed; Phase 07 atomic-publication owner), and `CELLOS-LOADER-CLEANUP-003` (Medium: PlatformCap singleton denial leaves an already-ready task and resources alive; Phase 07 denial-rollback/cleanup owner). None is fixed by this phase.

## Next Steps
Transfer loader ownership to Phase 07 for atomic task publication and denial cleanup under `CELLOS-LOADER-RACE-002` and `CELLOS-LOADER-CLEANUP-003`; Phase 03 retains `CELLOS-LOADER-SIG-001`. Phase 08 may consume the frozen v2 baseline only after Phase 07 qualification.

## Deviation Log
No contract deviations. Implementation was split into focused internal
`manifest_section` and `elf_manifest` modules to keep parsing bounded and each
new code file below 200 lines. Structural manifest classification runs before
the existing signature-section lookup so malformed section metadata cannot
reach the third-party ELF lookup; signature policy and manifest semantics are
otherwise unchanged. This ordering proves the narrow malformed-manifest gate,
not the adjacent pre-existing loader-security findings listed above.
