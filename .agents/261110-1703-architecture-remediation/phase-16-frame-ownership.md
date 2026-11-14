---
phase: 16
title: "Frame ownership and shootdown contract"
status: in-progress
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

- **Decision (step 1, 2026-11-14):** census in [research/p16-census.md](research/p16-census.md). The plan's claim (a) is false: at least seven conditions gate a free, and each extra one becomes a `Quarantined` reason. (b) Bounded drain is not true today; a per-CPU acknowledgement epoch will provide it. (c) Holds.
- **Decision (owner, 2026-11-14):**
  - F9: `ShmAlloc` zeroes its frame before handing it out.
  - F3: the x86 stack teardown loses its lower-half f→f kernel-RWX alias, and its flushes are batched.
  - The other defects the census found are fixed in this phase, each in its own commit with evidence: domain-grant rollback freeing unconfirmed frames, AArch64 VM teardown flushing only the VM loaded on the CPU, x86 vCPU creation leaks, and the DMA export lock-rank inversion.
  - The "flush under `FRAME_ALLOCATOR` does not compile" proof is a standalone rustc script run in CI. This adds a file outside the S7c file set.
- **Correction (step 1):** the drop order checks pins and leases held by other cells *before* unpublishing. The dead per-task `grant_table` was already deleted in `e88613a26b`; only a stale doc comment remains.
- **Decision (step 2, commit 1):** `memory/frame_run.rs` implements the census §7.1 types. `RunState` carries no payload in `Mapped`/`Unpublished`: the publications and pending waits live with the owner and the `Batch`, not in every run. The invalidation reasons carry no epoch set either: the pending entry holding the runs records it once (a set is `MAX_HARTS` words, too large to copy into every run). Epoch-keyed reasons (`TagUnconfirmed`, `SharedUnconfirmed`, `RfenceFailed`) arise only from an unpublished run whose flush was unconfirmed, and only a published epoch ends them; event-keyed reasons (`Pinned`, `IotlbUnacked`, `DomainRevoking`, `VmLive`) arise only from a `Mapped` run and only `Pending::release_event` ends them, returning the run to `Mapped`. An invalid transition is an `Err`, never a panic. Dropping a run that is not `Free` leaks it (counted), never frees it.
- **Decision (step 2, commit 1):** the pending list keeps one entry per wait (per tag, one shared, one RFENCE-fallback, one per event key) and merges by per-hart maximum epoch. Merging only waits longer, and it bounds the reaper's per-tick cost (census risk 8) to entries × `MAX_HARTS` loads with a small entry count. A run whose `VaHold` is set keeps its virtual range reserved until the frames are freed (census risk 2).
- **Deviation (step 2, commit 1):** `memory::pin::ReleaseKey` becomes `pub` (it was private) so `Reason::Pinned` can carry it, as the census specifies.
- **Decision (step 2, commit 2):** `FRAME_ALLOCATOR` is private to `memory/frame.rs`. Callers use `frame::lock(&mut FlushCap) -> AllocGuard<'_>`, `frame::with(impl FnOnce(&mut FrameAllocator) -> R + Send)`, or single-operation accessors (`allocate_frame`, `allocate_contiguous[_aligned]`, `deallocate_frame[s]`, `stats`, `install`, test-only `replace`). The guard borrows the cap for its whole life (a flush under it is E0499). The `with` closure must be `Send` and `FlushCap` is `!Send`, so the closure cannot capture a cap (E0277).
- **Deviation (step 2, commit 2):** the census's per-hart move-only `FlushCap` singleton (`take() -> Option`, one cap per hart) is replaced by a zero-sized `!Send` cap that `FlushCap::take()` refuses only while *this hart holds the allocator*. Kernel contexts can migrate between harts while holding no lock, so a per-hart singleton held across a preemption point would refuse caps to the next context on the old hart. The allocator lock masks interrupts, so the per-hart "held here" flag is exact. What remains open is a context that mints two caps before locking; the test-hooks shared-flush assert (commit 3) catches it at runtime.
- **Deviation (step 2, commit 2):** the rank-71 runtime assert moves to commit 3, where the shared flush takes the cap. Until then, x86 still flushes under the allocator (F2), and the assert would fail every test-hooks lane.
- **Deviation (file set, commit 2):** making the allocator private touches every file that named it. These are 45 files outside the S7c §3 set: arch boot, selftests, p01, snapshot, syscall telemetry, and the loader tests. Each change is mechanical (same lock scope, same operation); the only reordering is that two boot-time registrations (`main.rs` bitmap span, snapshot scratch) now run just after the allocator lock instead of under it. Neither registry is guarded by that lock. `scripts/check-frame-guard-compile-fail.sh` (owner-approved addition) runs in `ci.yml` job `board-configs`, next to the HAL boundary check. `scripts/lanes.toml` lists lane scripts only (not `check-*` gates), so it has no row.
- **Decision (step 3, commit 3):** `tlb_shootdown::flush_page/flush_range/flush_ranges(&mut FlushCap, …) -> FlushOutcome`. Shared flushes never panic. x86: a probe that hits its bound returns `Unconfirmed{epochs}`. RV64: an RFENCE error falls back to the IPI epoch path (counted, and logged by the hart-0 reaper, because the flush may run under `KERNEL_ROOT`, which ranks above the console). The arch halves move to `arch/*/tlb.rs::shared_flush_local`, which lowers the HAL ratchet from 704 to 692. `frame_run::Batch` holds one operation's invalidation ranges and retired runs, and `finish(cap)` issues one shared flush for all of them. `complete()` uses the hart's cap; a hart that holds the allocator gets the local flush plus remote requests with no wait, so its runs quarantine. Dropping a batch completes it. The test-hooks assert that the allocator is not held now lives in `flush_ranges`.
- **Deviation (step 3, commit 3):** census row 4 put "x86 `map_page`/`unmap_page` return leaves" into commit 4. It lands here, because a capped flush cannot be issued under the allocator, where `map_page` used to flush. x86 `map_page` records only a replaced present leaf (x86 caches no not-present entry). Detached tables on a failed walk become `PageTable` runs in the batch. RV64/AArch64 map/unmap leave the batch untouched (their HAL unmap already invalidates locally or by broadcast), and callers keep their explicit range invalidations, so the scopes there are unchanged. The `Stack` and `CellSegments` teardowns, the stack-allocation failure paths, the grant/SHM releases, the loader unwind and the MMIO maps each became one batch here as well, because their old per-page flushes ran under the allocator. The loader unwind's tables now get a shared flush instead of a local one (census P10).
- **Decision (step 3, commit 3):** these permission-lowering callers fail closed. `protect_page`/`protect_pages` return `InvalidationUnconfirmed` and log `shared invalidation unconfirmed`. W^X then refuses the spawn, and the unwind quarantines the frames. CapRevoke lowers the MMIO PTEs *before* it releases the registry windows, so a window whose lowering is unconfirmed stays registered to the target, the capability stays, and a retry lowers again. New-leaf/raise callers (MMIO map, ELF map, framebuffer, `#PF`) log and proceed. Stack allocation, grant allocation and `ShmMap` treat an unconfirmed invalidation of a *replaced* leaf as a failed allocation or map.
- **Surprise (step 3, commit 3):** the retirement path `resource_registry::release_mmio_for` (RV64/AArch64 only) releases ownership *before* it lowers the PTEs. That ordering predates this phase. If its lowering is unconfirmed (only possible after an RV64 RFENCE error *and* a silent hart), the window is released while a stale USER entry may survive on that hart. It is logged and was a panic before. Keeping the window owned would need a quarantine state in the MMIO registry, so this is left for the owner.
- **Decision (step 3, commit 3):** the hart-0 timer reaper also calls `frame_run::reap()` for every `cellos_wide` build (`task.rs`, a file outside the S7c set). Without it a quarantined run would never be freed.
- **Decision (step 3, row 4, MMIO release):** a window whose release lowering is unconfirmed stays owned; this closes the commit-3 Surprise. `release_mmio_for` now lowers every USER page of the task's windows *before* it releases any of them, with `REGISTRY` held throughout as before. It then releases only the windows whose lowering is in force on every hart. A window with a failed or unconfirmed page passes to the `QUARANTINED_OWNER` sentinel (no task id is `usize::MAX`). `lookup_mmio_owner` then reports no owner, every overlapping claim is refused (the `request_mmio` handover from a Terminated owner included), and no later release frees the window in that boot. The window is counted (registry entries under the sentinel) and logged once, after `REGISTRY` is released. A page covered only by a quarantined window no longer keeps USER when another window is released: it has no owner to keep it for.
- **Surprise (step 3, row 4, MMIO release):** `release_mmio_for` (RV64/AArch64) takes `KERNEL_ROOT` and the console while it holds `RESOURCE_REGISTRY`, which ranks after both. `request_mmio` likewise takes `SCHEDULER` under it. Both predate this phase. No test-hooks lane retires an MMIO-owning task, so the rank checker has never seen them. They are left as they are: dropping `REGISTRY` before the lowering would let a Terminated-owner handover race the PTE update.
