---
phase: 05
title: Stop Slow Death — Frame/PT Reclaim + Async-Pin GC
priority: P0
status: planned
depends_on: ["03"]
risk: high
---

# Phase 05 — Stop Slow Death (Memory Reclamation)

> ⚠️ **Red-team revisions (authoritative — override conflicting text below):**
> **This phase is now P0 and MUST land before Phase 04 enables auto-restart.** It is also more
> dangerous than originally scoped — in a SAS with no MMU, a wrong free = silent system-wide
> corruption.
> - **No ownership tracking exists.** `metadata.rs` is a 15-line header; grant/lease track *VA
>   ranges* not frame ownership ([tcb.rs:64-78](../../kernel/src/task/tcb.rs)); `AsyncLocked` does
>   not exist anywhere. **Prerequisite (do first):** build an explicit **per-cell mapping log** —
>   record every frame a cell creates (ELF segments, stacks, `ShmMap` entries) at spawn/map time.
>   No `free_range` call may exist until this log + an asserted "shared/granted" flag is live.
> - **Single shared page table — there are NO per-cell PT pages to free.** `satp` is set once and
>   never changes ([paging.rs:218-222](../../kernel/src/memory/paging.rs)); all cells identity-map
>   into ONE table. **Delete the "free page-table pages" step.** Free only the **leaf PTEs** this
>   cell created (from the mapping log); NEVER free intermediate PT pages (corrupts all cells).
>   State this single-PT invariant in the phase so per-cell-PT thinking isn't reintroduced.
> - **Async-pin GC is build-from-scratch**, not "complete the registry". Re-price step 4 to large.
> - Fix file references: there is no usable "metadata registry"; do not cite `AsyncLocked`.

## Context Links
- Spec: [12-reliability.md](../../docs/specs/12-reliability.md) §4.4
- Code: [kernel/src/memory/frame.rs](../../kernel/src/memory/frame.rs) (bitmap allocator, no Drop)
- Code: [kernel/src/memory/cell_quota.rs](../../kernel/src/memory/cell_quota.rs) (heap refund OK)
- Code: [kernel/src/task/scheduler.rs](../../kernel/src/task/scheduler.rs) (`exit_task`, zombies)
- Code: [docs/specs/03-runtime.md](../../docs/specs/03-runtime.md) (async pinning / AsyncLocked)

## Overview
- **Priority:** P1
- **Status:** planned
- **Description:** Today only heap quota is refunded on cell death; **physical frames, page
  tables, and async-pinned buffers are not reclaimed**. A robot running 24/7 with the new
  auto-restart (Phase 04) will crash-restart cells repeatedly — each leak accumulates until
  OOM kills the system. This phase makes cell teardown fully reclaim memory (Law 8: RAII/Drop).

## Key Insights
- The crash→restart loop turns a *slow* leak into a *fast* one — Phase 04 makes this phase
  necessary, not optional.
- Zombies persist in `scheduler.zombies` until the next `pick_next`; there is no full teardown
  of their frames/page tables. Need an explicit **reaper** that frees a zombie's resources.
- Async-pinned buffers (`AsyncLocked` in the metadata registry) have **no owner after a
  crash** — they need a GC keyed by owning cell id: when a cell dies, force-release its pins.
  Requires the metadata registry to actually track owner→pin (currently incomplete).

## Requirements
**Functional**
- On cell teardown: free all physical frames it owns (segments, stacks, page-table pages).
- Page-table pages allocated for a cell are freed (walk + free, or track per-cell PT frames).
- Async-pinned buffers owned by a dead cell are released so the frames return to the allocator.
- Heap quota deregister stays (already correct) — extend, don't duplicate.

**Non-functional**
- Reclamation must not free frames still referenced by a *surviving* cell (shared grants).
  Respect the grant/lease ownership before freeing.
- Reaping should be bounded work per pass (don't stall the scheduler); can be incremental.

## Architecture
```
On exit_task / terminate_on_fault (mark zombie):
   record cell_id for reaping

reaper (called from idle or a low-prio kernel task):
   for zombie z:
     1. release async pins owned by z.cell_id   (metadata registry: drop AsyncLocked by owner)
     2. free user segment frames                 (from the cell's load record)
     3. free stack frames (incl. guard frame)    (Stack::Drop)
     4. free page-table pages for z              (per-cell PT frame list, or PT walk)
     5. cell_quota::deregister(z.cell_id)        (already done — keep)
     6. remove from registry / zombies

Track per-cell owned frames at spawn so reclamation is O(owned) not a full walk.
```
Prefer **Law 8 Drop**: give `Stack`, and a new `CellMemory` record, `Drop` impls that free
frames; the reaper just drops the zombie's owned records.

## Related Code Files
**Modify**
- `kernel/src/task/stack.rs` — implement `Drop` for `Stack` (free frames incl. guard).
- `kernel/src/memory/frame.rs` — ensure a safe `free_frame`/`free_range` exists for Drop paths.
- `kernel/src/task/scheduler.rs` — reaper pass; teardown integration; zombie removal after reap.
- `kernel/src/loader.rs` / `kernel/src/task.rs` — record per-cell owned frames (segments, PT)
  at spawn for later reclamation.
- `kernel/src/cell/registry.rs` or metadata registry — owner→async-pin tracking + release-by-owner.

## Implementation Steps
1. Add a per-cell `owned_frames` / `CellMemory` record populated at spawn (segments, stacks, PT).
2. Implement `Drop for Stack` to free its frames (guard included); remove ad-hoc leaks.
3. Implement `free_range` in the frame allocator if missing; make idempotent + asserted bounds.
4. Implement owner-keyed async-pin release in the metadata registry (complete the OwnerID/State
   tracking referenced in 03-runtime.md / 02-memory.md).
5. Add a reaper invoked from the idle loop (or a low-priority kernel task) that drops a zombie's
   `CellMemory`, releases pins, deregisters quota, removes the zombie.
6. Guard against freeing shared/granted frames still owned by a live cell (check lease/grant).
7. Build; run a boot→crash→restart loop N times; confirm free-frame count returns to baseline.

## Todo List
- [ ] Per-cell owned-frame record at spawn
- [ ] `Drop for Stack` frees frames (+ guard)
- [ ] `free_range` in frame allocator
- [ ] Owner-keyed async-pin release (complete metadata registry)
- [ ] Reaper pass (idle/low-prio) + zombie removal
- [ ] Shared/granted-frame safety check
- [ ] Test: kill+restart a cell 100× → allocator free count stable (no monotonic leak)

## Success Criteria
- A scripted loop that spawns→crashes→restarts a cell many times shows **flat** free-frame
  usage (within noise), proving no per-restart frame/PT leak.
- Async buffer pinned by a cell that then crashes is reclaimed (frame returns to allocator).
- No double-free / use-after-free regressions (existing tests pass; add targeted tests).

## Risk Assessment
- **Use-after-free / double-free when reclaiming shared frames (High).** Freeing a frame still
  referenced by a live cell corrupts the SAS silently. *Mitigation:* free only frames provably
  owned by the dead cell; consult grant/lease tables; add asserts; bisect with a
  free-then-poison debug mode.
- **Reaper races the scheduler (Med).** Freeing a zombie mid-switch. *Mitigation:* reap only
  fully-descheduled zombies under the scheduler lock; never reap `current`.
- **Metadata registry was incomplete (Med).** Completing owner tracking is non-trivial.
  *Mitigation:* scope to owner→pin map needed for GC; defer full address-range registry.

## Security Considerations
- Frame zeroing on free/realloc (kernel already zeroes new frames) prevents data leakage from a
  dead cell to a future cell. Ensure reclaimed frames are zeroed before reuse.

## Next Steps
- Phase 06 (optional) hardens realtime guarantees on the now-stable base.
