---
phase: 4
title: "Reserve a proven contiguous boot heap"
status: pending
priority: P1
effort: "1 day"
dependencies: [1]
tier: medium
---

# Phase 04: Boot heap contiguity

## Requirements / architecture
`kernel/src/main.rs:570-590` obtains one frame and then 1,023 more via next-fit `allocate_frame`, but initializes 4 MiB at `phys_to_virt(first_frame)` as if all were adjacent. The allocator's contiguous search exists at `kernel/src/memory/frame.rs:316-372`; the unsafe assumption can overwrite unowned/reserved frames after a fragmented map. Keep heap size and virtual mapping policy unchanged; reserve exactly `HEAP_FRAMES` contiguous 4 KiB pages in **one allocator transaction** and fail boot explicitly before heap setup if no such run exists. Never fall back to partial allocation or a smaller silent heap.

## Related files
`kernel/src/main.rs`, `kernel/src/memory/frame.rs`, host allocator tests in `kernel/src/memory/frame.rs`, board boot smoke scripts.

## Implementation steps
1. Test a fragmented bitmap with enough free total frames but no 1,024-frame run, and one with a run after a gap. Assert no partial use-count changes on failure and that the returned interval is fully marked used. Use test-sized parameter rather than allocating 1,024 pages in host fixture.
2. Replace the boot loop with `allocate_contiguous(HEAP_FRAMES)` or a measured reserve-range method that cannot claim reserved frames. Check allocator lock hold/time on a large-but-fragmented map; if scan exceeds boot bounds, add a free-run scan optimization without changing semantics.
3. Run fresh RV64, AArch64 and x86 boot-to-shell smoke on normal and fragmented boot maps (emulated map fixture as needed). Verify adjacent frame addresses and heap length via a test-only readout, not a textual assertion on source code.

## Success criteria
- [x] Heap span is exactly a reserved consecutive 4 MiB run; fragmented map without a run fails before `init_heap` without corrupting unrelated memory.
- [x] All supported paged targets boot normal image; heap accounting and frame total stay exact. *(RV64 and AArch64 verified; the x86_64 lane is unavailable here — see Deviation Log.)*

## Progress

### Slice 1 — the boot heap is one reserved run (2026-09-27) — done

`main.rs` reserved the 4 MiB heap by calling `allocate_frame()` 1,024 times and then
initializing the heap at the *first* frame's address as if all of them were adjacent. That
only holds while the allocator's next-fit cursor walks free memory linearly: on a fragmented
map the reservation spans frames the kernel does not own, and `init_heap` then writes a 4 MiB
heap over them.

- `memory::frame::reserve_contiguous_run(allocator, frames)` is the boot path now — one
  `allocate_contiguous` transaction, so either the whole run is reserved and marked, or
  nothing is marked at all.
- `main.rs` calls it for `HEAP_FRAMES` and panics with an explicit message when no run exists,
  **before** `init_heap`: a smaller or scattered heap is never a fallback.
- Host tests (`cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu`): a map with
  more free frames than the run needs but no run at all (every 100th frame taken) must either
  reserve exactly the contiguous span or mark nothing — red before the fix at frame 50, where
  the old body's scattered reservation covered a frame that was already taken; and a run after
  a 64-frame gap is reserved whole from the first free frame with exact accounting.

Verified: host lane 113 passed / 0 failed; RV64 test-hooks boot (`admission`, `asid-lease`)
green; AArch64 test-hooks boot reaches `[vfs-test] Results: 96 PASS, 0 FAIL` and exits cleanly
(the lane's `admission-core` marker is red for a pre-existing quota interaction documented in
`phase-02-domain-root-lifetime.md` § Progress — it is not a boot failure and not caused by this
change); RV64 production `launch-profile` green. The x86_64 boot smoke is not runnable in this
environment (no Limine ISO tooling), so that target's lane stays unexecuted rather than claimed.

## Assumptions / risk / rollback
- [UNVERIFIED] Boot allocator may have other early frame consumers before heap creation; inspect the 570-line boot ordering and inject a fragmented bitmap. Rollback: restore previous image only if its boot memory map is proven contiguous, otherwise stop boot; an overwritten frame cannot be restored by image rollback. Heap perf improvement is not claimed until measured.

## Deviation Log

- **Fragmented map proved on the host, not in a boot image.** The plan allows "an emulated map
  fixture as needed": the reservation contract (exactly the run, or nothing marked) is proved
  by the host tests, and the boot path itself is proved on the two runnable targets. A boot
  image with a deliberately fragmented firmware map would need its own lane and is not claimed.
- **x86_64 boot smoke not run** (no Limine ISO tooling in this environment). Its heap path is
  the same `reserve_contiguous_run` call, but that is an argument, not a witness.
- **Subagent delegation remains unavailable** (Codex provider quota), so this slice's review
  was a session self-review.
