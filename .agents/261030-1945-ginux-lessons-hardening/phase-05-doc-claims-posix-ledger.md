---
phase: 5
title: "Doc claim checker and POSIX ledger"
status: pending
priority: P2
effort: "3d"
dependencies: [1]
tier: medium
---

# Phase 05: Doc claim checker and POSIX ledger

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Overview

Two machine checks, both modeled on ginux `tools/architecture/check.sh:8-50` and `check-linux-compat.sh:8-78`:

1. A claims checker. It requires decided statements to stay present and rejects obsolete statements, so retired claims cannot quietly return.
2. The POSIX symbol ledger that ADR-0018 already asks for (`docs/decisions/0018-*.md:120-139`). It is cross-checked against the shim and the mlibc sysdeps, so "fail loudly with ENOSYS" (Spec 05 §3.3) is verifiable.

## Requirements

- Claims checker: rules in `docs/doc-claims.toml` as `{file, require|reject, text, reason, decided_by}`. `decided_by` must be an ADR, spec version or maintainer decision. CI fails on a violation.
- Initial rules: only statements already decided by an accepted ADR/spec. Examples: ADR-0022 Intel-only direction text in README; rejecting the retired viFS1/viFS2 policy wording outside historical notes. Rule candidates come from ADR "Supersedes"/"Superseded" sections, not from guesses.
- POSIX ledger: `docs/posix-ledger.md` with one row per entry point: `symbol | header | provider (shim/mlibc/none) | status Complete/Partial/Policy-denied/Missing | spec or test ref`, plus a totals line.
- Ledger checker: every symbol exported by `libs/api/src/services/posix/**` and every sysdep implemented in `third_party/mlibc/sysdeps/vicell/**` must have a row. A row marked `Missing` must not be implemented. Totals must match. Order and duplicates are validated.

## Assumptions

- **Claim:** mlibc sysdeps for vicell are enumerable from `third_party/mlibc/sysdeps/vicell` source (function definitions `sys_*`). **Confidence:** medium. **How to verify:** `grep -rn '^int sys_\|^[a-z].* sys_' third_party/mlibc/sysdeps/vicell`.
- **Claim:** shim exports are `#[no_mangle] pub extern "C" fn`. **Confidence:** medium. **How to verify:** grep `no_mangle` under `libs/api/src/services/posix`.

## Related Files

- Create: `scripts/check-doc-claims.py`, `docs/doc-claims.toml`, `scripts/check-posix-ledger.py`, `docs/posix-ledger.md`, `scripts/test_doc_checks.py`.
- Modify: `.github/workflows/ci.yml` (two steps next to `check-spec-anchors`), `docs/decisions/0018-*.md` (link the ledger as the published table), `docs/specs/05-application.md` §3.3 (link).

## Implementation Steps

1. Claims checker plus tests (a required text missing → fail; a rejected text present → fail; a rule without `decided_by` → fail; a historical block opted out by an `<!-- historical -->` fence → ignored).
2. Seed the rules from accepted ADR supersession statements. Each rule cites its decision.
3. Generate the initial ledger by enumerating the shim exports and sysdeps. Set status per symbol from the existing tests/specs. Unverified behavior is `Partial`, never `Complete`. `Complete` requires a positive and a negative test reference.
4. Ledger checker plus tests (an unlisted implemented symbol → fail; `Missing` but implemented → fail; totals mismatch → fail).
5. CI steps; links from ADR-0018 and Spec 05.
6. The README vs controlling-plan status conflict (scout report) gets a rule **only after** the maintainer's Phase 01 decision.

## Success Criteria

- [ ] `python3 scripts/check-doc-claims.py` and `python3 scripts/check-posix-ledger.py` exit 0; `python3 -m unittest scripts/test_doc_checks.py` passes, with all negative fixtures failing for the stated reason.
- [ ] Every rule has `decided_by`; every ledger `Complete` row has positive and negative test refs.
- [ ] CI runs both checks.

## Security Considerations

The ledger makes `Policy-denied` explicit, so a denied call cannot drift into silent approximation.

## Risk Notes

The rule set can rot into string-matching busywork: keep it to decided supersessions and review its size each quarter. Undo: remove the CI steps.

## Deviation Log

None.
