# Phase 01 evidence — truthful status baseline

**Date:** 2026-11-10 · **Base:** `8a005f998` + working tree (uncommitted) · **Ceiling:** host (scripts, cargo metadata). No build, boot or test of product code was needed or run.

## Observed state of the controlling plan (input to the correction)

| Phase | Frontmatter before | Unchecked / checked boxes | Set to |
|---|---|---|---|
| P01 | complete | 8 / 0 | in-progress |
| P02 | complete | 5 / 0 | in-progress (partial code) |
| P03 | complete | 7 / 0 | pending |
| P04 | complete | 5 / 0 | in-progress (partial code) |
| P05 | complete | 6 / 0 | pending |
| P06 | complete | 8 / 0 | pending |
| P07 | complete | 5 / 0 | pending |
| P08 | complete | 5 / 0 (gates A–E) | pending |
| P09 | complete | 5 / 0 | pending |
| P10 | complete | 11 / 0 | pending |

Counts: `grep -c '^\s*- \[ \]'` / `grep -c '^\s*- \[[xX]\]'` per file. The plan body already carried a 2026-10-16 correction withdrawing completion; commit `805f458c6` (same day) set the header/table/portfolio to complete.

Code checks (read this session):
- `dispatch_handle_control` / `dispatch_cspace_control`: callers only in `kernel/src/cell/cspace.rs` tests (1431-1511) and `kernel/src/cell/provenance_selftest.rs:37`.
- Production `cspace::` use: `task/launch.rs:486,566`, `task/scheduler.rs:613,685`, `task/syscall.rs:7148` and BootFS file-ticket arms `8114-8454`.
- `cells/services/{budget-authority,workload-router,placement-controller}`: the only `#[cfg(test)]` markers are `extern crate std` in `main.rs:5`; zero `#[test]`; no reference from `scripts/`, `.github/`, `config/`, `tools/`, `tests/` or the x86 image manifest.

## Checks

| Check | Result |
|---|---|
| `python3 scripts/check-plan-status.py --quiet-legacy` | exit 0; 32 legacy files reported (41 before the controlling-plan correction) |
| Negative control: temp `261111-0000-x/phase-01-a.md` with `status: complete` + one `- [ ]` under Success Criteria | `FAIL …`, exit 1 |
| Positive control: same file with `- [x]` in Success Criteria and `- [ ]` under an unrelated heading | exit 0 |
| `python3 -m unittest scripts/test_check_spec_anchors.py` | OK |
| `python3 scripts/check-spec-anchors.py --check` (after regenerating) | exit 0 |
| `cargo metadata --locked --format-version 1` | OK |
| `python3 scripts/check-cargo-config-parity.py` | exit 0 |
| `Cargo.lock` diff | removes exactly `service-budget-authority`, `service-placement-controller`, `service-workload-router` |
| grep "COMPLETED MILESTONE" / "all 10 phases" in portfolio and controlling plan | only inside the dated correction text that quotes it |
