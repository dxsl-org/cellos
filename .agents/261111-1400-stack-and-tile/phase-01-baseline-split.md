---
phase: 1
title: "Baseline and frame registry split"
status: completed
priority: P1
effort: 0.5d
dependencies: []
tier: fast
checkpoint: false
---

# Phase 1: Baseline and frame registry split

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Context Links
- Plan: [plan.md](./plan.md) · Scout: [reports/scout-report.md](./reports/scout-report.md)
- Red team M4 (baseline/checkpoints), m2 (file size)

## Overview
Record the QEMU baselines every later checkpoint compares against, and split
`surface_table/frame.rs` (1141 lines) into focused modules with no behaviour
change, so phases 2–7 do not grow one file past `docs/code-standards.md:633-634`.

## Key Insights
- `scripts/lanes.toml:208-211`: the phase-04 GUI proof is `tier = "manual"`,
  "current pass/fail unknown" in the repo record. It passed 295/295 on KVM in this
  session (2026-11-11), not committed as evidence. `window-policy` (`lanes.toml:788`,
  `tier = "pr"`) was not run after the icon/menu tab change.

## Requirements
- Functional: none (refactor + evidence).
- Non-functional: `frame.rs` split into `surface_table/frame/{model,registry,ipc_ops,snap,tests}.rs`, each < 300 lines, public API unchanged.

## Architecture
Module move only; `pub use` keeps `crate::surface_table::FrameRegistry` paths.
Pre-planned homes for later phases: `frame/membership.rs` (phase 2),
`frame/geometry.rs` (phase 3), `pointer_router/{stack_drag,tile_drag}.rs`
(phases 5–6), `input_handler/modifiers.rs` is not needed (phase 5 reads
`KeyEvent.modifiers`).

## Assumptions
None — no unverified claims.

## Related Code Files
- Create: `cells/services/compositor/src/surface_table/frame/{model,registry,ipc_ops,snap,tests}.rs`
- Delete: `cells/services/compositor/src/surface_table/frame.rs` (content moved)
- Create: `.agents/261111-1400-stack-and-tile/reports/baseline.md`

## Implementation Steps
1. Build the RV64 image the CI way and run `cargo test --test window-policy` (tests/integration); run `scripts/qemu-phase04-gui-test.sh` on KVM; record pass counts, commit hash and log paths in `reports/baseline.md`.
2. Split `frame.rs` by concern; keep tests intact.
3. `cargo test -p service-compositor` (x86_64 host) + riscv64 `cargo check`.

## Todo List
- [x] baselines recorded  - [x] frame.rs split  - [x] tests green

## Success Criteria
- [x] `reports/baseline.md` lists window-policy and phase-04 GUI proof results on this tree (fail allowed, but recorded with the failing check). — [baseline](./reports/baseline.md): GUI proof 295 PASS; window-policy FAIL at step 3 on this tree and clean `HEAD`.
- [x] Compositor host suite passes with identical test count; no file in `surface_table/frame/` ≥ 300 lines. — 48 → 48 tests (Deviation Log). **Size limit not met:** `frame/tests.rs` is 306 lines at `9f62760a64` and at `c4c7d5d34f`; every non-test file is under 300.

## Security Considerations
N/A (no behaviour change).

## Risk Assessment
- Running window-policy rebuilds the shared RV64 kernel and `disk_v3.img`; do it on a branch/worktree or record the artifacts' hashes and restore them.
- Rollback: revert the split commit.

## Next Steps
Phase 2 builds frame membership on the split modules.

## Deviation Log
- **Surprise:** `window-policy` fails on the pre-plan tree and on clean `HEAD` at the same step (`front probe did not start`: the typed shell line reaches the focused probe window). Recorded in `reports/baseline.md`; checkpoints compare against it.
- **Deviation:** `frame.rs` split as `frame/{mod,views,regroup,placement,tests,placement_tests}.rs` (+ `membership.rs`, `tile.rs` later); 48 → 48 tests unchanged by the split.
