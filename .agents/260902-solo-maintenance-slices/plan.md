---
title: "Solo Maintenance Slices Implementation Plan"
description: "Three bounded CellOS slices for truthful memory reporting and removal of two dead kernel APIs."
status: completed
progress: 100%
completed: 2026-09-02
priority: P2
effort: 6h
branch: main
tags: [bugfix, refactor, backend, tech-debt]
blockedBy: []
blocks: []
created: 2026-09-02
---

# Solo Maintenance Slices Implementation Plan

## Overview

Deliver exactly three approved, independently reviewable slices: authorize and wire both `free` surfaces to the existing `MemInfo` ABI, delete the dead cell-ID freeze registry, and delete the false unused `CapTable::grant_to` surface. No ABI allocation or redesign is included.

## Phases

| Phase | Name | Status |
|-------|------|--------|
| 1 | [Report Real MemInfo](./phase-01-report-real-meminfo.md) | completed |
| 2 | [Remove Dead FROZEN Registry](./phase-02-remove-dead-frozen-registry.md) | completed |
| 3 | [Remove False grant_to API](./phase-03-remove-false-grant-to.md) | completed |

## Completion Evidence

- `cargo fmt --all -- --check` exited 0. API host tests passed 91/91 with 4 ignored; kernel host tests passed 88/88. The focused MemInfo bit-56 and stale-reservation tests each passed 1/1.
- The repository-supported RV64 workspace check and clippy (excluding the five documented unsupported optional crates) exited 0, as did the release build for the affected kernel, apps, services, supervisor, and hotswap demos. The initial unexcluded workspace check failed only in those documented optional crates; it is not an affected-code regression.
- `CELLOS_INCLUDE_CAPACITY_PROBE=1 pwsh ./gen_disk.ps1` exited 0, signing 47 cells and producing a 16-file VIFS1 plus a 51-entry disk table containing `/bin/free` and `/bin/capacity-probe`. The tracked generated `kernel/src/embedded/init` was restored afterward with `git restore -- kernel/src/embedded/init` (exit 0), leaving no tracked generated binary modified. The capacity-observability QEMU test passed 1/1 in 8.88s with truthful shell and standalone rows, retained bit-56 denial, typed OOM, and post-OOM shell recovery.
- Both focused hotswap QEMU tests passed 1/1 with their required state-restore, retained-authority, cutover, FIFO/old-TID, and counter markers. All four clean-cutover searches returned their expected no-match/retained-live-symbol results. `git diff --check c1895c09 --` exited 0.
- Final independent review verdict: **CORRECT / safe to ship**, confidence 0.98, with no blocking or non-blocking correctness findings. Review covered the full 11-file diff against `c1895c09`, changed-file context, routing and lifecycle consumers, QEMU assertions, and disk packaging flow.

## Execution Order and Dependencies

- Review and land in numeric order for a simple audit trail; the phases have no technical dependency and must remain separate commits/slices.
- Within a slice, keep production-source changes separate from test/evidence or living-document projection changes. Do not manufacture a verification commit when no verification file changes.
- This plan neither depends on nor modifies `260902-six-ordered-follow-up-lanes`; its POSIX, diagnostic AArch64, and pinned-QEMU scopes remain untouched.

## Global Boundaries

- Keep `ViMemInfoV1`, syscall id 243, allowlist bit 56, allocator semantics, and default-deny opt-in policy unchanged.
- Preserve task-incarnation hotswap state, replacement ceilings/reservations, mailbox cutover, and rollback behavior.
- Do not replace `grant_to` with delegation, sharing, a new capability kind, or a new ABI.
- Runtime QEMU results are scoped regressions, not production-capacity or hardware evidence.
