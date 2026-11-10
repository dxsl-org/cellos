---
phase: 19
title: "Spec reconciliation, hygiene and closure"
status: pending
priority: P3
effort: "5–7d"
dependencies: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18]
tier: medium
checkpoint: true
---

# Phase 19: Spec reconciliation, hygiene and closure

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Context Links

[Review Medium docs/governance, Low](../reports/architecture-review-261110.md) · [Scout traceability](scout-report.md) · `docs/specs/` · `docs/system-architecture.md` · `docs/spec-status.generated.md` · `scripts/check-spec-anchors.py` · `CONTRIBUTING.md` · `Dockerfile`

## Overview

Bring the specs in line with the code the earlier phases produced, remove the remaining dead residue, and re-run the review's checks to close each finding with evidence or an explicit owner-accepted residual.

## Key Insights

- Verified: spec number 14 is used twice; `docs/system-architecture.md` and `current-focus.md` lagged the plan flip; `.agents/` holds 193k tracked Markdown lines.
- Reviewer-cited contradictions remain after the per-phase spec edits unless swept: Spec 03 (IPC "removed"), 07 (zero-copy handoff), 09 (trait/`Future` contracts), 10 (absent test files), 18 ("no domain branch"), 19 (Layer B "Planned"), 22 (x86 "refused").
- Per-arch status is restated in four specs; one generated table avoids repeat drift.

## Requirements

- Functional: every spec either matches code or carries a dated "historical" banner; per-arch implementation status in one generated table referenced by Specs 02/18/19/22; duplicate spec number resolved by renaming one file with link fix-ups; Low items removed; `.agents/` retention rule applied (archive completed plans' `reports/`, delete stale ones) only with owner approval; closure matrix: finding → phase → evidence file → status.
- Non-functional: no evidence file altered; archive moves preserve history.

## Architecture

- `scripts/generate-arch-status.py` → `docs/generated/arch-status.md`, driven by cfg/feature probes and lane results in `scripts/lanes.toml` (lane tier and last recorded ceiling), not prose.
- Closure matrix lives in `reports/closure-matrix.md` and is what the status guard reads for this plan.

## Assumptions

- **Claim:** `check-spec-anchors.py` tolerates a renamed spec file once anchors are updated.
  **Confidence:** medium
  **How to verify:** read the script; run it after the rename.
- **Claim:** junk files at the root are unreferenced (`invalid.json`, `dummy.jsonl`, the `sha256:…jsonl` file, `__build*.bat`, `do_build.bat`, `doom_build_debug.txt`, `test_subprocess.py`, `test_python_payload.py`).
  **Confidence:** medium
  **How to verify:** grep each name across `scripts/`, `.github/`, `tests/`.
- **Claim:** the owner wants the `.agents/` retention rule applied retroactively.
  **Confidence:** low
  **How to verify:** ask; default is leave untouched and only report sizes.

## Related Code Files

- Modify: `docs/specs/{02,03,04,07,09,10,15,16,17,18,19,22}-*.md`, `docs/system-architecture.md`, `docs/roadmap/current-focus.md`, `docs/codebase-summary.md`, `docs/project-changelog.md`, `docs/project-roadmap.md`, `CONTRIBUTING.md`, `Dockerfile`, `llms.txt`, `rust-toolchain.toml` / `deny.toml` (target names), `tests/integration/.cargo/config.toml`
- Create: `scripts/generate-arch-status.py`, `docs/generated/arch-status.md`, `reports/closure-matrix.md`
- Delete: root junk listed above; `kernel/src/memory.rs` dead items; duplicate `PAGE_SIZE` definitions; stray `cells/services/compositor/kernel-graphics-legacy.rs`; any residue named in the review's Low list not already removed

## Implementation Steps

1. Build the closure matrix from `scout-report.md`; for each finding link the phase evidence and re-run its original check (the grep/count the review used).
2. Sweep specs: fix or banner; move per-arch status to the generated table.
3. Resolve the duplicate spec number; update `llms.txt` and anchors.
4. Delete Low-list residue with a reference grep before each deletion.
5. Fix `CONTRIBUTING.md`, `Dockerfile`, target-name mismatches.
6. Report `.agents/` size and ask the owner about retention; apply only what is approved.
7. Update living docs (`system-architecture.md`, changelog, roadmap, code standards where patterns changed).
8. Final verification and closure request to the owner; lock exceptions for this plan end.

## Todo List

- [ ] Closure matrix
- [ ] Spec sweep + generated arch status
- [ ] Spec numbering
- [ ] Residue removed
- [ ] Contributor docs fixed
- [ ] Retention decision
- [ ] Living docs
- [ ] Owner closure

## Success Criteria

- [ ] Every Critical and High row in the closure matrix is `closed` with an evidence link, or `residual` with the owner's written acceptance; no row is `closed` on prose alone.
- [ ] `python3 scripts/check-spec-anchors.py` and `python3 scripts/check-plan-status.py` exit 0.
- [ ] Review re-check numbers recorded beside the originals (e.g. `target_arch` outside `arch/`, path literals in kernel, `sys_send` in services, tracked binaries >1 MB).
- [ ] Checkpoint: all `pr` and `nightly` lanes in `lanes.toml` green on the closing commit; independent end-to-end review (haily-reviewer) recorded.

## Risk Assessment

- Marking a spec section historical can hide a still-valid requirement: banner only sections contradicted by code that an earlier phase deliberately produced; anything else is a new finding, logged.
- Deleting plan history is irreversible for working trees without the commit: archive by `git mv`, delete nothing without approval.

## Security Considerations

`docs/security-model.md` must end the plan describing the real authority rule, posture set and Tier-1 perimeter; it is re-read against Phases 04, 06, 07 and 18.

## Next Steps

Hand back to the portfolio: remaining open items of the 2026-11-05 security audit and the physical-qualification gates, which this plan did not touch.

## Deviation Log

None.
