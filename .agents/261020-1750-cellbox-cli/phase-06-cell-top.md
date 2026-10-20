---
phase: 6
title: "Cell-aware telemetry and top"
status: pending
priority: P1
tier: thinking
dependencies: [3, 4, 5]
---
# Phase 06: Cell-aware top

## Overview and requirements

Upgrade existing top, not a replacement fake monitor. CBX-17/18 require incarnation-aware Cell default view, task drill-down, honest sampling/memory/completeness and equivalent TUI/batch/record data.

## Architecture and related files

Reuse `cells/tools/shell/src/top.rs`, its renderer and `libs/text-engine/src/top*` through the P04 moved modules. Resolve actual task/Cell identity and resource ownership with the approved telemetry provider before editing `libs/api/src/abi/syscall.rs`, `libs/ostd` wrappers or kernel task telemetry. Kernel/API changes are owned and gated by P01, not app-private inferred fields.

## Implementation steps

1. Supply authoritative incarnation identity, task membership, timestamp/counter units and completeness/paging through an admitted provider. A name group or bare reusable Cell ID is insufficient. Add an approved versioned provider delta only if existing contracts cannot supply these facts.
2. Project Cell and task records separately. Count shared Cell heap once, distinguish per-task stack/segments from allocator committed capacity and never label unsupported values RSS.
3. Compute CPU deltas for the same incarnation/membership over valid monotonic intervals; represent new/reset/stale/unavailable samples explicitly. Apply the P01-defined core-equivalent or machine-normalized convention; no accidental per-task clamp on a multi-core Cell.
4. Replace the silent 64-row adapter ceiling with complete bounded paging/frame metadata or explicit truncation. Bound per-frame memory; filter/sort each sample, not an infinite history.
5. Expose finite observation and streaming record modes; implement top Ratatui view with field selection/sort and task drill-down. Batch mode is a renderer over the same numeric records, no key reads or ANSI in machine output.
6. Exercise a real multi-thread Cell with known work/idle intervals, a restart/ID reuse, denied telemetry and enough tasks to exceed the former cap. Compare all render modes for one recorded snapshot and verify no double-counted memory.

## Success criteria

- [ ] Cell incarnations remain distinct across restart/reuse; drill-down returns their actual tasks.
- [ ] Shared memory is not summed repeatedly; CPU convention and unknown samples are explicit.
- [ ] Missing/denied/truncated telemetry is not presented as zero load or complete inventory.
- [ ] Interactive, batch and typed consumers agree for the same snapshot; observation grants no lifecycle rights.

## Verification

Keep deterministic delta/lifetime/aggregation/boundary tests and exercise the actual target provider plus real top interaction. Existing tests pinning headings are not proof. Record actual data provenance; no CPU-speed or physical hardware claim follows from QEMU work counters.

## Assumptions

Admitted providers can expose incarnation/membership/resource scope. Current ProcessInfoV2 lacks identity, so absence is an explicit prerequisite, not a parser workaround. Confirm exact approval/source before changes.

## Security and risk assessment

Cross-Cell observations are permissioned; do not broaden telemetry visibility by adding TUI. Rollback requires a matched consumer/provider/image version; never reinterpret new rows as old ABI bytes. Observation output is irreversible disclosure, so enforce scope before sampling.

## Deviation log

None; implementation pending. Record decisions/deviations/surprises with reason, impact and revert/escalation.
