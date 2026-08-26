---
phase: 4
title: "Lock Per-Request Cell Scale Profile"
status: complete
priority: P1
effort: "3h"
dependencies: [3]
tier: thinking
---

# Phase 4: Lock Per-Request Cell Scale Profile

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Overview

Close D5 by making the per-request server profile a first-class goal with explicit prerequisites, measurement gates, and rollback wording. Do not implement image sharing or demand-paged stacks in this plan.

## Requirements

- Functional: PDR and specs must withdraw bare "1000+ Cells" and replace it with profile-specific scale targets.
- Non-functional: The target must name the memory model that makes it plausible: DTB memory discovery, shared read-only image frames, demand-paged stacks, smaller quotas, and raised tables after measurement.

## Architecture

Observed current anchors:
- `docs/specs/19-hardware-isolation-layers.md:88` through `docs/specs/19-hardware-isolation-layers.md:105` already names the large-app and per-request profiles.
- `docs/project-overview-pdr.md:521` still says bare `Support 1000+ Cells`.
- `kernel/src/memory/cell_quota.rs:15` keeps `MAX_CELLS = 64`, and `kernel/src/memory/cell_quota.rs:22` sets the default quota at 16 MiB.
- `kernel/src/loader/va_alloc.rs:48` sets `MAX_SLOTS = 512`.
- `kernel/src/loader/elf.rs:273` through `kernel/src/loader/elf.rs:282` copies ELF bytes into freshly allocated frames per page load.

Data flow for the future implementation: ELF image bytes enter loader, segment metadata splits mutable and immutable pages, immutable `.text`/`.rodata` map through image-hash refcounted frames, per-cell mutable heap/stack/quota remain private, and MemInfo exports per-spawn delta at N = 64/128/256/512.

## Assumptions

- **Claim:** A1 DTB parsing and A3 MemInfo are already present in the current dirty tree.
  **Confidence:** high
  **How to verify:** grep `ViMemInfoV1`, `MemInfo`, and DTB memory-node parser before implementation.

## Related Files

- Modify: `docs/specs/19-hardware-isolation-layers.md`
- Modify: `docs/project-overview-pdr.md`
- Modify: `docs/project-roadmap.md`
- Modify: `docs/TODO.md`
- Create: future implementation plan for image sharing + demand-paged stacks, if absent

## Implementation Steps

1. Make recommendation A explicit: per-request server profile is accepted.
2. Replace bare `1000+ Cells` with measurable profile gates: N, per-spawn memory delta, spawn latency, and isolation property.
3. Queue implementation order: shared immutable image frames, demand-paged stacks, dynamic cell/quota tables.
4. State backwards compatibility: large-app profile and existing `MAX_CELLS = 64` remain default until measurements support raising.
5. Record D5 as ruled in the docket/report.

## Success Criteria

- [x] PDR no longer contains an unexplained `1000+ Cells` target.
- [x] Spec 19 names accepted prerequisites and measurement gates.
- [x] Existing large-app behavior remains the default.
- [x] No runtime code changes are made by this docs closure phase.

## Security Considerations

Shared immutable frames are acceptable only for read-only segments after W^X lowering. Mutable data, heap, stacks, grants, and quotas must remain per cell.

## Risk Notes

- Likelihood medium, impact high: image sharing can accidentally share mutable relocation/data pages. Mitigation: future implementation must prove segment permissions and refcounts before mapping shared frames.
- Rollback: revert docs/plan edits and keep large-app-only wording. Irreversible part: none.

## Deviation Log

None.
