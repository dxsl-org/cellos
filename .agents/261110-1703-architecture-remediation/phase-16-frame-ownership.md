---
phase: 16
title: "Frame ownership and shootdown contract"
status: pending
priority: P1
effort: "10–14d"
dependencies: [15]
tier: thinking
checkpoint: true
---

# Phase 16: Frame ownership and shootdown contract

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Context Links

[Review H-11, Medium IPC (region tables)](../reports/architecture-review-261110.md) · `kernel/src/memory/{frame,address_space,deferred_release,pin,tlb_shootdown,paging}.rs` · `kernel/src/task/{stack,dma_export,domain_grant}.rs` · `kernel/src/task/user_copy/sas-lease.rs` · `kernel/src/hypervisor/svm_registry.rs` · [Spec 02](../../docs/specs/02-memory.md) · [Spec 19](../../docs/specs/19-hardware-isolation-layers.md)

## Overview

One owner type for runs of physical frames whose `Drop` performs the release protocol, and one invalidation contract for shared and private roots that never panics on a slow CPU.

## Key Insights

- Verified: the x86 shared-root path panics when remote invalidation is unconfirmed (`tlb_shootdown.rs:460-465`) after a 2e9-tick wait (`:378`). Private roots already probe-then-defer for the same event (`deferred_release.rs:14-24`).
- Reviewer-cited: RAII `OwnedFrame` is used in three files; raw `PhysAddr` alloc/free in 17 non-test files (`svm_registry.rs` 18 sites, `paging.rs` 13); retain-until-safe implemented six separate ways; the "identity-mapped kernel-RWX free frame" invariant is restated by each releaser; "no flush under `FRAME_ALLOCATOR`" is a comment while `release_frames` requires that lock.
- Five region tables (`SHM_HANDLES`, `PAGE_GRANT_TABLE`, `REG_GRANT_TABLE`, DMA `EXPORTS`, domain grants) each own frames with their own lifetime rules.

## Requirements

- Functional: `FrameRun` (owner incarnation, range, state `Mapped | Unpublished | Quarantined(reason) | Free`) whose drop order is unpublish → flush (batched, once per operation) → wait for open leases → normalise → free; on unconfirmed invalidation frames move to `Quarantined`, loudly logged, never reused, never a panic; a guard type makes "flush while holding `FRAME_ALLOCATOR`" unrepresentable; stacks, cell segments, grants, SHM, DMA exports, EPT/Stage-2 tables and VMCB pages migrate onto it; one `Region` record (owner, rights, pin state, receivers) behind the five tables; `CellSegments` moves from `task/stack.rs` to `memory/`.
- Non-functional: no change to syscall-visible grant ids; teardown cost O(operation) broadcasts, not O(pages).

## Architecture

```
FrameRun::drop ─► unpublish PTEs (all roots that map it)
               ─► flush batch ─► confirmed? ─ yes ─► wait leases ─► normalise ─► FRAME_ALLOCATOR.free
                                     └ no ─► Quarantined(reason) ─► retry on later epochs ─► free | stay
```

`AddressSpace` keeps its ledger; it stores `FrameRun`s instead of bare frames.

## Assumptions

- **Claim:** every retain-until-safe site waits for the same three conditions (no PTE, no TLB entry, no lease/pin/DMA use).
  **Confidence:** low
  **How to verify:** tabulate the six implementations' conditions before designing the state machine; any extra condition becomes a `reason` key.
- **Claim:** quarantined frames on a healthy system drain within a bounded number of epochs.
  **Confidence:** medium
  **How to verify:** add a counter and observe on the 4-CPU x86 stress lane.
- **Claim:** hypervisor frames (EPT roots, guest RAM) can adopt the type without touching VM-entry hot paths.
  **Confidence:** medium
  **How to verify:** read alloc/free sites in `svm_registry.rs` and `memory/{ept,stage2}.rs`.

## Related Code Files

- Create: `kernel/src/memory/frame_run.rs`, `kernel/src/memory/region.rs`, `kernel/src/memory/cell_segments.rs` (moved)
- Modify: `kernel/src/memory/{frame,address_space,deferred_release,pin,tlb_shootdown,paging,ept,stage2}.rs`, `kernel/src/task/{stack,dma_export,domain_grant}.rs`, grant/SHM syscall arms, `kernel/src/hypervisor/{registry,svm_registry}.rs`, `docs/specs/{02-memory,19-hardware-isolation-layers}.md`
- Delete: per-site quarantine lists once migrated; the dead per-task `grant_table`

## Implementation Steps

1. Tabulate the six release protocols and their lock orders; design states and reasons from the union.
2. Implement `FrameRun` with host tests for every state transition and the lock-guard type.
3. Replace the x86 shared-root panic with quarantine; batch per-page flush callers (`CellSegments` teardown, `map/unmap_page_x86`).
4. Migrate owners in order of risk: stacks → cell segments → grants/SHM → DMA exports → domain grants → hypervisor tables. One owner per commit, stress lane after each.
5. Introduce `Region` behind the five tables; retire SHM onto Grant where callers allow (three callers, two are fault-injection).
6. Update Spec 02 §3 and Spec 19 (x86 shootdown exists; Layer B is implemented).

## Todo List

- [ ] Protocol table and design
- [ ] `FrameRun` + host tests
- [ ] No-panic shootdown, batching
- [ ] Owners migrated
- [ ] `Region` record
- [ ] Specs updated

## Success Criteria

- [ ] x86 fixture: with one CPU held with interrupts masked past the wait bound, a cell teardown completes, frames are reported quarantined and later freed — the kernel does not panic (panics before this phase).
- [ ] Host tests cover each `FrameRun` transition; holding the allocator lock across a flush does not compile.
- [ ] Raw `PhysAddr` free calls outside `memory/frame*.rs` reduced to zero (grep).
- [ ] Checkpoint: full `pr`-tier suite on three arches; x86 4-CPU stress and domain grant suites; hypervisor smoke lane; review recorded.

## Risk Assessment

- A wrong drop order frees a live frame: the highest-severity failure in the plan. Mitigation: migrate one owner at a time, keep each site's old debug asserts until its lane passes, never weaken a wait.
- Quarantine can leak memory under a persistently silent CPU: surfaced as a counter and a log, bounded by refusing new allocations — a degraded node, not corruption. Rollback: per-owner revert.

## Security Considerations

Frames reused under a live mapping are the class behind audit H-2 and H-5. The contract here is "refuse or quarantine, never reuse".

## Next Steps

Phase 17 attaches quota ownership to `FrameRun` and hardens the shared root.

## Deviation Log

None.
