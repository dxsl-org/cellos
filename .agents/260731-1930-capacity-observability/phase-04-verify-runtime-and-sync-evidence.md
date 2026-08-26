---
phase: 4
title: "Verify runtime truth and synchronize evidence"
status: completed
priority: P1
effort: "3h"
dependencies: [2, 3]
tier: medium
---

# Phase 4: Verify Runtime Truth and Synchronize Evidence

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log when it occurs.

## Overview

Prove the ABI on every target, exercise real OOM and MemInfo behavior on RV64, then update records with the measured result even when it fails the old target.

## Requirements

- Functional: typed OOM/logging and MemInfo benchmark work end to end; successful spawning and normal boot remain unchanged.
- Non-functional: final tests run against final code; failures are fixed and rerun, never waived.

## Architecture

Verification layers: API/allocator/syscall unit tests, four-target compile/typecheck, bounded RV64 allocation-pressure probe, normal RV64 shell/benchmark boot, then specialist tester and reviewer passes. The runtime report separates “mechanism works” from “10 MiB target passes.”

## Assumptions

- **Claim:** current integration infrastructure can select guest RAM size for the OOM probe.
  **Confidence:** high
  **How to verify:** A1 added the memory-size builder used by its 2 GiB capacity gate; inspect the live helper before writing the fixture.

## Related Files

- Modify: `tests/integration/src/lib.rs` only if the existing memory-size helper is insufficient
- Create/Modify: focused A2/A3 integration test and bounded probe fixture
- Modify: `.github/workflows/perf.yml` only after the metric ruling; do not hide a real FAIL
- Modify: `docs/performance-report.md`
- Modify: `docs/project-changelog.md`
- Modify: `docs/project-roadmap.md`
- Modify: `docs/TODO.md`
- Modify: `.agents/reports/decision-docket-260730.md`

## Implementation Steps

1. Run focused API, allocator, and syscall tests; compile RV32, RV64, AArch64, and x86_64.
2. Run a bounded RV64 spawn-pressure probe and require typed OOM, source/summary logs, no panic, and a responsive shell.
3. Run normal RV64 shell and benchmark gates; require a MemInfo-derived value and benchmark completion.
4. Record the actual allocator-committed value and whether `<10 MB` passes. Do not change the threshold or CI grep merely to preserve green status.
5. Delegate final verification to `haily-tester`; fix and rerun until passing within the ruled metric semantics.
6. Delegate production-readiness review to `haily-reviewer`; resolve findings, then synchronize living docs through `haily-project-manager`/docs writer as warranted.
7. Run `git diff --check` and inspect `git diff --cached --name-only`; do not stage or commit concurrent IPC/A1/A4 edits.

## Success Criteria

- [x] Unit and supported-target compile gates pass; RV32 retains documented pre-existing portability failures.
- [x] RV64 OOM is typed and logged; normal spawn/boot remains healthy.
- [x] Benchmark reports real allocator-committed bytes and completes.
- [x] CI/docs state the real result without redefining the target silently.
- [x] Final verification has no unresolved mechanism-critical findings.
- [x] Shared index and unrelated worktree edits remain untouched.

## Security Considerations

Runtime logs and reports expose only aggregate counts and bounded labels. Test fixtures must not weaken production policy or add a production bypass.

## Risk Notes

The likely 10 MiB failure is evidence, not a regression in measurement machinery. Undo code through a focused revert; published measurements and ABI allocations cannot be made “unused” retroactively and must remain documented.

## Deviation Log

- **2026-08-01 — Result:** Runtime verification passes for the A2/A3 mechanism. The measured
  allocator-committed footprint is 135,782,400 bytes (129.49 MiB), so the `<10 MiB`
  performance objective fails honestly and moves to a separate optimization follow-up.
- **2026-08-01 — Safety:** The destructive `capacity-probe` is included and signed only for
  test-mode images built with `CELLOS_INCLUDE_CAPACITY_PROBE=1`; default images exclude it.
