---
phase: 1
title: "Close ADR Documentation"
status: completed
priority: P1
effort: 1d
dependencies: []
tier: medium
---

# Phase 01: Close ADR Documentation

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Overview

Stabilize and commit the current ADR/spec/changelog edits before integrating branch commits that also touch docs. This isolates predictable conflicts and makes the trust-tier decision auditable.

## Requirements

- Functional: finalize `docs/specs/18b-cell-admission-consent-adr.md` and its references from Spec 18, Spec 19, and changelog.
- Non-functional: no code changes; no claim that admission/consent is implemented.

## Architecture

Data flow: draft ADR text enters docs review -> cross references in Spec 18/19 are normalized -> changelog records the decision -> commit becomes the stable base for later cherry-picks.

## Assumptions

- **Claim:** The untracked ADR file is intentional user work, not throwaway scratch.
  **Confidence:** medium
  **How to verify:** ask user if deletion/rename is considered; otherwise preserve and commit as first-class doc.

## Related Files

- Modify: `docs/project-changelog.md`
- Modify: `docs/specs/18-cell-trust-tiers.md`
- Modify: `docs/specs/19-hardware-isolation-layers.md`
- Create: `docs/specs/18b-cell-admission-consent-adr.md`

## Implementation Steps

1. Re-run `git status --short --branch` and confirm no non-doc dirty files entered the worktree.
2. Review ADR for explicit "not implemented" language and Layer-B gating.
3. Verify Spec 18 links to ADR and keeps current admission behavior explicit (`docs/specs/18-cell-trust-tiers.md:60-75`, `:141-151`).
4. Verify Spec 19 references the installer Tier-1/Tier-2 gate (`docs/specs/19-hardware-isolation-layers.md:140-143`).
5. Run markdown/link grep for `18b-cell-admission-consent-adr.md`.
6. Commit only these four docs with a focused message.

## Todo List

- [x] Dirty worktree contains only the four ADR/spec/changelog files.
- [x] ADR says owner consent is a future admission store, not today's `/POLICY.BIN`.
- [x] Spec 18 and Spec 19 references resolve by relative path.
- [x] Commit created before integrating `b5a97125` or `eecfbb72`.

## Success Criteria

- [x] `git status --short` is clean after commit, excluding later implementation work.
- [x] `git show --stat HEAD` contains only the four doc files.
- [x] Changelog entry says "Nothing here is implemented" or equivalent.

## Evidence

- `git status --short --branch` → `## main...origin/main [ahead 1]`; ` M docs/project-changelog.md`; ` M docs/specs/18-cell-trust-tiers.md`; ` M docs/specs/19-hardware-isolation-layers.md`; `?? docs/specs/18b-cell-admission-consent-adr.md`
- `git diff --stat -- '.agents/260805-1833-midori-closure-execution'` → no plan-dir diff.
- `git show --stat --oneline --name-only af9a9a8e` → exactly four files: `docs/project-changelog.md`, `docs/specs/18-cell-trust-tiers.md`, `docs/specs/18b-cell-admission-consent-adr.md`, `docs/specs/19-hardware-isolation-layers.md`.

## Security Considerations

Do not weaken fleet admission language. The phase only documents the split between build-time attestation and install-time consent; it must not claim a new trust anchor exists.

## Risk Notes

| Risk | Likelihood x Impact | Mitigation | Rollback |
|------|---------------------|------------|----------|
| ADR overclaims implementation | Medium x High | Explicit "not implemented" language | Revert doc commit |
| Doc conflict with pending commits | High x Medium | Land docs first, resolve once | Revert or reorder doc commit before cherry-picks |

## Backwards Compatibility

Docs-only. No runtime, ABI, or user-facing command behavior changes.

## Deviation Log

None.
