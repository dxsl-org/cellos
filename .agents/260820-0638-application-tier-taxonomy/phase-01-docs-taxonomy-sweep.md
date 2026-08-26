# Phase 01 - Docs Taxonomy Sweep

## Context Links

- `docs/decisions/0003-application-tier-taxonomy.md`
- `docs/app-development-guide.md`
- `docs/specs/05-application.md`
- `docs/specs/18-cell-trust-tiers.md`
- `docs/system-architecture.md`
- `docs/security-model.md`
- `docs/project-roadmap.md`
- `docs/roadmap/runtime-and-platform-tracks.md`
- `docs/roadmap/product-stages.md`

## Overview

Priority: P2. Status: completed. Effort: 5h. Tier: thinking.

Normalize active documentation around three application tiers, runtime profiles,
named SDK modules, and G1-G5 product stages. Preserve historical changelog,
legacy roadmap, research, and guide filenames for link compatibility.

## Key Insights

- ADR/specs own decisions; roadmap/guides own current implementation status.
- `Tier 1b` is useful as a legacy search term but must not remain a distinct
  execution class in active docs.
- `layer` remains valid for system/hardware implementation layers, not app tiers.

## Requirements

- Functional: active docs explain Tier 1/2/3, runtime profiles, SDK modules.
- Non-functional: no product-code edits, no ABI changes, no guide path renames.
- Backwards compatibility: old `tier1b-*` and `tier3b-*` filenames remain.

## Architecture / Data Flow

User-facing docs enter through `app-development-guide.md`, flow to Spec 05 for
taxonomy, Spec 18 for trust/admission, and roadmap pages for G-stage status.
The transformed output is consistent terminology across those active docs.

## Related Code Files

None modified.

## Implementation Steps

1. Add ADR 0003 recording the accepted taxonomy.
2. Update app guide and Spec 05 as developer-facing entrypoints.
3. Update Spec 18/security/system architecture to preserve containment warnings.
4. Update roadmap pages so G4 `std` is a runtime profile and G5 is a platform overlay.
5. Mark legacy guide names as aliases without renaming paths.

## Todo List

- [x] Add ADR.
- [x] Update main active docs.
- [x] Preserve history/legacy docs untouched.
- [x] Finish legacy-term cleanup in secondary active docs and example paths.
- [x] Run final grep/link sanity after cleanup.

## Success Criteria

- `Tier 1b` appears only as legacy alias/profile wording in active docs.
- `SDK L1` no longer appears in active docs.
- Tier 2 is never described as shipped or merely "unsigned Tier 1".

## Risk Assessment

- Medium x Medium: stale historical docs still show old terms. Mitigation:
  intentionally exclude changelog/research/legacy from active-doc consistency.
- Medium x High: weakening untrusted-code warning. Mitigation: keep security
  model explicit that Tier 2 is not implemented.
- Undo: revert only docs touched in this phase.
- Irreversible: none; published terminology may still influence readers after
  merge, so changelog/release notes should mention it if shipped.

## Security Considerations

Do not imply C/FFI/Lua is safe for hostile code in Tier 1. Trusted runtime
profiles share the SAS.

## Next Steps

Proceed to non-breaking code terminology aliases only after approval.

## Evidence

- `rg -n "Tier 1b|Tier 3b|SDK L1|SDK layer|2026-08-19" docs/...` showed only legacy aliases in active docs and no active `SDK L1`.
- `git status --short` showed the docs/plan files updated in this turn, with no code files changed by this task.
