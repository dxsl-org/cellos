---
phase: 1
title: "Define IPC Benchmark Semantics"
status: complete
priority: P1
effort: "2h"
dependencies: []
tier: thinking
---

# Phase 1: Define IPC Benchmark Semantics

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Overview

Make D1b explicit: the 50 us IPC target is either a hardware requirement with QEMU regression tracking, or a QEMU release gate. The current source mixes those meanings.

## Requirements

- Functional: Bench output and docs must state whether `TARGET_IPC_NS = 50_000` gates QEMU CI or hardware qualification.
- Non-functional: Do not weaken regression detection; separate absolute targets from emulator baselines.

## Architecture

Observed data flow: `IpcSendRecvBench` produces samples, `BenchReport::meets_target` compares p99, and `main.rs` increments failed count on miss. Evidence: `cells/tests/bench/src/main.rs:39`, `cells/tests/bench/src/main.rs:41`, `cells/tests/bench/src/main.rs:233`, `cells/tests/bench/src/main.rs:238`.

The bench contract should split:
- absolute hardware target: PDR requirement for qualified hardware;
- QEMU-TCG contract: report p50/p99 and fail only on configured regression threshold unless a QEMU-specific target has been measured and ratified.

## Assumptions

- **Claim:** CI consumes the bench binary's failed count directly.
  **Confidence:** medium
  **How to verify:** grep workflow/scripts for bench invocation and nonzero exit handling before editing.

## Related Files

- Modify: `cells/tests/bench/src/main.rs`
- Modify: `docs/performance-report.md`
- Modify: `docs/project-overview-pdr.md`
- Modify: `docs/project-roadmap.md`

## Implementation Steps

1. Verify all bench callers and CI consumers.
2. Pick the contract: recommended ruling is hardware target + QEMU regression gate.
3. Update bench constants/comments/output so IPC mirrors the existing syscall TCG/hardware split.
4. Update PDR and performance docs to say p99 is still the statistic, but QEMU-TCG is evidence, not hardware qualification.
5. Record D1b as ruled in the docket/report.

## Success Criteria

- [x] Bench source no longer implies the same 50 us IPC target applies to both QEMU-TCG and hardware.
- [x] PDR target remains visible and measurable.
- [x] QEMU failure semantics are documented and traceable to CI.
- [x] `git diff --check` passes.

## Security Considerations

N/A.

## Risk Notes

- Likelihood medium, impact high: changing pass/fail semantics can hide a real regression. Mitigation: keep regression comparison active and name the baseline owner.
- Rollback: revert the bench/docs edits. Irreversible part: none; only text and local gate semantics change.

## Deviation Log

None.
