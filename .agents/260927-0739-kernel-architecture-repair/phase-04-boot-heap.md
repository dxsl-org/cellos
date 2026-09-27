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
- [ ] Heap span is exactly a reserved consecutive 4 MiB run; fragmented map without a run fails before `init_heap` without corrupting unrelated memory.
- [ ] All supported paged targets boot normal image; heap accounting and frame total stay exact.

## Assumptions / risk / rollback
- [UNVERIFIED] Boot allocator may have other early frame consumers before heap creation; inspect the 570-line boot ordering and inject a fragmented bitmap. Rollback: restore previous image only if its boot memory map is proven contiguous, otherwise stop boot; an overwritten frame cannot be restored by image rollback. Heap perf improvement is not claimed until measured.

## Deviation Log
None.
