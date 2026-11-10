---
phase: 1
title: "Truthful status baseline"
status: completed
priority: P1
effort: "3–4d"
dependencies: [0]
tier: medium
checkpoint: true
---

# Phase 01: Truthful status baseline

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Context Links

[Review C-2](../reports/architecture-review-261110.md) · [Controlling plan](../261011-1200-cell-contract-authority/plan.md) · [Portfolio](../plan-portfolio.md) · [System architecture](../../docs/system-architecture.md) · [Current focus](../../docs/roadmap/current-focus.md) · [Red team](reports/red-team.md) #6, #9

## Overview

Make the controlling plan, portfolio and architecture docs state what the code does, and add a guard so a phase cannot be `complete` with unchecked criteria. Closes C-2.

## Key Insights

- Verified: `plan.md:4,78-87` of the controlling plan says all ten phases complete; its body says pending (`:206,228,238`); `phase-10-qualification.md:4` is `complete` with `[ ]` at `:45-49`.
- Verified: `cells/services/{budget-authority,workload-router,placement-controller}/src/main.rs:25-31` are heartbeat loops; no consumer outside root `Cargo.toml`.
- Historical evidence stays historical: correct status, do not delete records.

## Requirements

- Functional: per-phase observed state for P01–P10 with a code pointer or "no implementation"; one consistent status across plan, portfolio, `system-architecture.md`, `current-focus.md`; skeleton services handled per owner decision 1.
- Non-functional: no evidence file rewritten; corrections are appended with date and reason.

## Architecture

- Status correction table in the controlling plan: `claimed → observed → evidence`.
- Guard `scripts/check-plan-status.py`: for every `.agents/*/phase-*.md` and `plan.md` with `status: complete|completed`, fail if a `- [ ]` remains under Success Criteria or Todo. Start as report-only for plans created before 2026-11-10, enforcing for this plan.
- Decision 1 default: remove the three crates from workspace `members` and from any image list; keep source; add a crate-level doc line "design skeleton, not wired, fail-open — do not package".

## Assumptions

- **Claim:** the three crates have no tests and no image/script consumer.
  **Confidence:** medium (lead grep found only `Cargo.toml` and the crates themselves; reviewer found zero `#[test]`; commit `cf4a10181` mentions host tests)
  **How to verify:** `grep -rn "#\[test\]\|#\[cfg(test)\]" cells/services/{budget-authority,workload-router,placement-controller}` and grep the crate names in `scripts/`, `.github/`, `config/`.
- **Claim:** CSpace is production-used only for BootFS file tickets.
  **Confidence:** medium (reviewer-cited `task/syscall.rs:8094-8160`, `cell/cspace.rs:830,938`)
  **How to verify:** list non-test callers of `cspace::` and the two control dispatchers.

## Related Code Files

- Modify: `.agents/261011-1200-cell-contract-authority/plan.md`, `phase-01..10-*.md` (frontmatter + appended correction), `.agents/plan-portfolio.md`, `docs/system-architecture.md`, `docs/roadmap/current-focus.md`, `docs/project-changelog.md`, root `Cargo.toml` (decision 1), the three crates' `lib.rs` doc line
- Create: `scripts/check-plan-status.py`, `reports/phase-01-evidence.md`

## Implementation Steps

1. For each of P01–P10, read the phase file's success criteria and locate the implementing code; record observed state with `file:line` or "none".
2. Append the correction table to the controlling plan; set each phase's `status:` to the observed value; set plan `status: in-progress`; remove `completed:`.
3. Replace `plan-portfolio.md:29` "COMPLETED MILESTONE" with the corrected summary and a link to the table.
4. Update `system-architecture.md` §11 and `current-focus.md` so both match the table; bump their "Updated" dates.
5. Apply owner decision 1 to the three crates; confirm the workspace still resolves (`cargo metadata --locked`).
6. Write `scripts/check-plan-status.py`; add it to the pre-build guard block of `ci.yml` next to `check-spec-anchors.py`.
7. Changelog entry; evidence file with the per-phase table.

## Todo List

- [x] P01–P10 observed-state table
- [x] Controlling plan, portfolio, docs aligned
- [x] Skeleton crates per decision 1
- [x] Status guard in CI
- [x] Evidence file

## Success Criteria

- [x] `python3 scripts/check-plan-status.py` exits 0 on this plan and reports (not fails) legacy violations, each listed.
- [x] The guard fails on a fixture phase file with `status: complete` and one `- [ ]` (negative control).
- [x] Grep for "all 10 phases" / "COMPLETED MILESTONE" finds no uncorrected claim in `.agents/plan-portfolio.md` or the controlling plan header.
- [x] `cargo metadata --locked --format-version 1` succeeds after the workspace edit.

## Risk Assessment

- Some phases are genuinely partly implemented (P02 arena, P04 operations): record "partial" with pointers rather than "pending". Rollback: `git revert`; documents only, plus one workspace edit.
- Guard noise on 190 legacy plan directories: report-only mode avoids blocking CI.

## Security Considerations

Budget Authority is the declared enforcement TCB; excluding the fail-open skeleton from images removes the chance of it being packaged as if it enforced anything.

## Next Steps

Phase 02. All later phases are subject to the status guard.

## Deviation Log

- Deviation: the three skeleton crates were moved to workspace `exclude`, not only removed from `members`.
  Why: a crate directory under the workspace root that is neither member nor excluded makes cargo refuse to build it standalone.
  Impact: root `Cargo.toml` `exclude`.
  Revert: move the three paths back to `members`.
- Deviation: `Cargo.lock` changed (three packages pruned); not listed in the S0 file set.
  Why: cargo prunes non-member packages on the next resolve; leaving it would make the lockfile stale.
  Impact: `Cargo.lock` only, 30 lines removed.
  Revert: restored together with the `Cargo.toml` revert.
- Deviation: `docs/spec-status.generated.md` regenerated; not listed in the S0 file set.
  Why: generated projection of the portfolio/plan status this phase changed; `check-spec-anchors.py --check` fails until regenerated.
  Impact: generated file only.
  Revert: regenerate after reverting.
- Decision: `docs/roadmap/current-focus.md` left unchanged.
  Why: it already states the ten-phase DAG is pending; nothing contradicted the corrected status.
  Impact: none.
  Revert: n/a.
- Decision: guard section match covers headings that start with `Todo` or contain `Success Criteria`/`Acceptance Criteria`.
  Why: the controlling plan uses "Verification / success criteria"; the first version missed it.
  Impact: `scripts/check-plan-status.py`.
  Revert: n/a.
- Surprise: the controlling plan body already contained a 2026-10-16 correction withdrawing completion; only header, table, frontmatter and portfolio disagreed.
  Why: commit `805f458c6` the same day.
  Impact: correction appended rather than reconstructed.
  Revert: n/a.

## Evidence

[reports/phase-01-evidence.md](reports/phase-01-evidence.md).
