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

- [x] Protocol table and design
- [x] `FrameRun` + host tests
- [x] No-panic shootdown, batching
- [ ] Owners migrated
- [ ] `Region` record
- [ ] Specs updated

## Success Criteria

- [x] x86 fixture: with one CPU held with interrupts masked past the wait bound, a cell teardown completes, frames are reported quarantined and later freed — the kernel does not panic (panics before this phase).
- [x] Host tests cover each `FrameRun` transition; holding the allocator lock across a flush does not compile.
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
- **Decision (step 3, row 4, quarantine cap):** the 1/64 cap is checked in `FrameAllocator::allocate_frame`, `allocate_contiguous` and `allocate_contiguous_aligned`, and in `frame::allocate_guest_ram`. Every caller (grant, SHM, spawn, VM, private root) sees its existing out-of-memory result. Each refusal is counted (`frame_run.refused`) and logged on powers of two. `FrameAllocator::allocate_frame_uncapped` is exempt from the cap, and every page-table frame in `memory/paging.rs` uses it: kernel-root tables, the normalisation remap of a freed frame, and the boot roots. The dormant x86 `#PF` demand frame is exempt as well, because that path panics on `None` and the cap must never turn a quarantine into a panic. Host tests read a per-thread quarantine count instead of `PENDING`.
- **Decision (review of commits 1–3):** an independent review found eight defects, fixed together in one commit:
  - The spawn unwind (`task/elf_prepare.rs`) hands its cell VA slot to its last retired run through a `VaHold`, using the new `Batch::hold_va`. The slot is now released only when the unwound frames are freed, not before their flush confirms.
  - `Batch::complete_without_wait` never frees: a hart that holds the allocator would deadlock in `frame::with`. Every run is quarantined behind the epochs requested (none on one hart, so the next reap frees it). The new cause `UnconfirmedCause::NotAwaited` logs its own line, at warn level and without the "invalidation unconfirmed" text that the lanes gate on. `KernelBackend::{normalise, free}` assert `!held_here()`.
  - AArch64 `shared_flush_local` issues `dsb ishst` before a full flush. `flush_all` lost `nomem`, and it adds `tlbi alle2is` at EL2, matching the per-page `vae2is`. Under VHE, `vmalle1is` already targets EL2&0, so `alle2is` is a belt.
  - An RV64 full flush now uses one RFENCE over the whole address space (`size = usize::MAX`) and falls back to epochs only on an error.
  - `Batch::invalidate_range` widens to a full flush when the range wraps the address space.
  - A failed alias restore (`normalise` → `false`, out of page-table frames) never frees the run. The run waits under a new reason, `Reason::AliasUnrestored` (reaper-ended, zero epochs). The reaper retries it on a later pass, never in the same one, and logs on powers of two. The restore path completes its batch without waiting, because the reaper runs in the timer ISR.
  - The W^X comment in `loader/wx.rs` is corrected. The `revoke_mmio_for` doc comment was already fixed in the MMIO commit.
- **Decision (step 3, row 4, fixture):** the x86 test-hooks fixture `S22-X86-QUARANTINE-SILENT-CPU` runs right after the shootdown fixture whenever 2+ CPUs are online. The RT CPU clears IF and spins on `X86_PROBE_STAGE` alone (stages 10–13 continue the shootdown fixture's), with no lock and no log, bounded by eight probe bounds of TSC. The BSP builds a user stack, a kernel stack, a three-page cell image at a fixture VA, and a two-page grant run. It retires them into one `Batch` through the new test-hooks helpers `Stack::retire_into`, `CellSegments::retire_into` and `test_retire_grant_pages_into` (the drops now share their bodies). It then asserts the following. The flush is `Unconfirmed{Timeout}` on the RT CPU after at most three bounds. Every data page is quarantined and none is freed or handed out again. The control: 40 reaper passes (one reissue) while the CPU is still silent free nothing. After the CPU is released, the reaper frees every page. The window is bracketed by `[frame-run] silent-cpu window OPEN/CLOSED`. The lane awk excuses only the fixture's own `… held (fixture)` line inside it, and both brackets must balance. The CONTROL and PASS markers are required at 2+ CPUs. No new global: the fixture reuses `X86_PROBE_STAGE`. `tag_probe_ticks` is `pub(crate)` on x86 for the fixture's timing assertion.
- **Decision (step 3, row 4, RV64 seam):** the test-hooks `TEST_FAIL_RFENCE` seam in `arch/riscv64/tlb.rs` makes every remote RFENCE report `SBI_ERR_FAILED` without sending it. A third round of the RV64 shootdown self-test arms the seam while it lowers `TEST_VA`. `protect_page` must succeed through the IPI epoch fallback, the RFENCE failure counter must move, and hart 1's next store must fault without changing the physical byte (`[selftest] TLB-SHOOTDOWN-RFENCE-FALLBACK: PASS`). `wx-cross-hart-tlb` now requires that marker. The regenerated `docs/generated/mutable-globals.md` also picks up the cap commit's host-test `thread_local!` row, which that commit missed.
- **Decision (step 4, row 5, stacks):** a `Stack` owns its frames as one `FrameRun`, `Mapped` from the allocation on. Every allocation failure path and the drop retire it: unmap, `unpublish`, then the batch. On x86 the run is `KernelAlias::Intact`, because RAM is reached through the HHDM. The teardown no longer restores an f→f kernel-RWX alias in the lower half (owner F3). Instead it prunes the lower-half tables that the stack's VA = PA mapping needed, into the same batch. RV64/AArch64 runs stay `Removed` (identity restore before free).
- **Decision (step 4, row 5, tag-unconfirmed stacks):** a domain task's teardown (`Task::drop`) and the thread-spawn rollback whose tag probe failed no longer `mem::forget` the stacks. That was a permanent leak, and it left the USER mapping in the shared root. They call `Stack::retire_behind_tag`. The shared-root teardown runs as usual, then `Batch::await_tag` quarantines the runs as `TagUnconfirmed{asid}`, whatever the shared flush says. Their epochs are asked for after that flush: the tag is flushed locally again, and every remote serves its epoch with a full flush. They are merged with any epochs an unconfirmed shared flush still owes. The reaper frees the runs, and their `STACK_RESERVATIONS` rows with them. Host test `a_run_behind_an_unconfirmed_tag_waits_for_a_fresh_epoch`.
- **Deviation (step 4, row 5):** `deferred_release::LEAKED_STACK_FRAMES` and `note_leaked_stack_frames` are deleted here, together with their last writers; census row 11 had listed them. Readers move to the `frame_run` counters: the RV64 deferred-release test's quiet check reads `leaked() == 0`, and the grant-gate accounting line now prints `run_quarantined=`/`run_leaked=` instead of `leaked_stacks=`. The test-hooks withheld-tag window still applies only to the `DEFERRED` queue. A stack quarantined behind a tag is freed on a real epoch, even inside that window (before this commit, it was never freed).
- **Decision (step 4, row 6, cell segments):** `CellSegments` and `release_cell_va` move unchanged from `task/stack.rs` to `memory/cell_segments.rs`, and every caller names the new path. `kernel/residents.toml` lists the new file. The teardown was already one batch, and the VA slot already rides on the last backing run's `VaHold`, so it is released only when the frames are freed (or stays quarantined with them). The remaining changes are two:
  - Detached page tables now require a **global** flush. `ReclaimedPageTables::retire_into(batch)` loses its `vaddr` argument and calls `Batch::invalidate_all`. On RV64, `SFENCE.VMA` with an address orders only the leaf entry, not the non-leaf entries a walk cached. That covers the loader unwind (census P10), the `CellSegments` teardown, the grant prune, the x86 stack prune and the failed-walk path of x86 `map_page`. On x86 nothing changes, because a shared flush there is already a full flush on every CPU. On RV64/AArch64, an operation that detaches a table now issues one full flush instead of per-page flushes.
  - `CellSegments::eager_unmap` (under `SCHEDULER`) sends its invalidation without waiting (`complete_without_wait`), which closes census F4. It frees nothing: the frames and the VA slot stay owned until the drop. The drop invalidates every VA again and frees them only once that flush confirms. No wait that guards a free is weakened.
