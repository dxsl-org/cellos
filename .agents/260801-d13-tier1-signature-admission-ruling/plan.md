# D13 Tier-1 signature-admission ruling

**Status:** Complete  
**Decision:** Recommendation A approved 2026-08-01  
**Evidence:** `../reports/d13-tier1-signature-admission-analysis-260801.md`

## Scope

- Separate default G1/dev admission from the future fleet-secure posture.
- Distinguish signature provenance, `/bin` authorization, F1 pipeline policy, and
  Tier-1/Tier-2 memory mapping.
- Correct production-key, signature-stripping, and Tier-routing status claims.
- Preserve all runtime behavior, keys, features, ABI, and concurrent worktree changes.

## Changes

- [x] Amend Specs 12 and 18 to state current loader behavior and future fleet gates.
- [x] Correct the security model's tampering, signed-only, and F1-attestation claims.
- [x] Synchronize directly implicated roadmap, changelog, and architecture status.
- [x] Mark D13 RULED/APPLIED in the report and docket.
- [x] Validate stale-claim searches, links, Markdown whitespace, and empty index.

## Success criteria

- Default builds are not described as signed-only or protected against signature stripping.
- The dev seed is described as a forgeable test fixture, not a provenance root.
- No document says signature status currently selects a memory tier.
- `/bin` is described as authorization classification, not cryptographic trust.
- Production acceptance criteria remain explicit and no runtime implementation is claimed.

## Evidence

- Normative corrections: `docs/specs/12-reliability.md`,
  `docs/specs/18-cell-trust-tiers.md`, and `docs/security-model.md`.
- Project-state synchronization: `docs/project-roadmap.md`,
  `docs/system-architecture.md`, and `docs/project-changelog.md`.
- Ruling record: `../reports/d13-tier1-signature-admission-analysis-260801.md` and
  `../reports/decision-docket-260730.md`.
- Validation: targeted stale-claim search reviewed, `git diff --check` clean, and the Git
  index remained empty. Documentation-only ruling; runtime tests were not required.
