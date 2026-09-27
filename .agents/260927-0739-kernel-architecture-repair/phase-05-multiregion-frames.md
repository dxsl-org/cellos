---
phase: 5
title: "Account for all usable physical-memory regions"
status: pending
priority: P2
effort: "split into map canonicalization and allocator migration"
dependencies: [4]
tier: thinking
---

# Phase 05: Multi-region frame allocator

## Requirements / architecture
`FrameAllocator::new_from_map` currently selects only the largest `Usable` map entry (`kernel/src/memory/frame.rs:46-69`; documented `docs/system-architecture.md:616-623`). Preserve reservation safety and bounded early-boot allocation while exposing **all** usable, page-aligned, non-overlapping ranges. Never map/acquire `Reserved`, MMIO, kernel/bootloader, ACPI or DTB-protected frames. Normalize/merge adjacent compatible entries with checked arithmetic; **halt** on a malformed authoritative Limine/firmware map, do not switch to a guess. Allow a target fallback **only if boot protocol absent** and the fallback has separately proved exclusions for the running kernel, DTB, bootloader, firmware and bitmap. Existing x86_64 `FALLBACK_MEMORY_MAP` incorrectly labels the region containing `kernel_phys_base=0x0020_0000` Usable (`kernel/src/boot.rs:699-720`); make that route fail closed until corrected/tested. Retain bounded early-boot representation (`boot::MAX_MEMORY_MAP_ENTRIES = 64`, `:93-99`).

## Related files
`kernel/src/{boot.rs,main.rs,snapshot.rs}`, `kernel/src/memory/{frame.rs,paging.rs}`, `kernel/src/task/syscall.rs` (MemInfo and framebuffer guard `:6533-6539`), `docs/{system-architecture.md,code-metrics.generated.md}` if generating metrics, boot and allocator tests.

## Implementation steps
1. Determine each consumer that assumes `memory_start()..memory_end()` is contiguous: snapshot headers/CRC/restore, guest contiguous allocator, allocation bitmap/snapshot iteration, `MemInfo`, framebuffer-MMIO exclusion, frame-index helpers. **Do not** just widen `memory_end` to the highest address. Preserve a `contains_managed_frame`/interval query for RAM vs MMIO guards.
2. Design fixed-capacity interval table plus per-interval bitmap or one compact bitmap with prefix-index mapping; prove physical↔index roundtrip, alignment, count overflow and frame lease accounting; bitmap frames themselves remain reserved and unavailable.
3. Implement allocate/contiguous allocate/deallocate across disjoint intervals; a contiguous request may not straddle a hole. Test holes, adjacent usable merge, malformed/overlapping entries, highest physical address, fragmented regions, double-free and uniform accounting. Test protocol absent vs malformed authoritative map separately: corrupted Limine must halt, and the audited fallback must prove exclusion of its live kernel image.
4. Port all contiguous-assuming callers to explicit per-range iteration/containment. `snapshot.rs` remains disabled until phase 07 defines its new versioned on-disk range inventory; no attempt to read old format as new. Verify `MemInfo` aggregate totals and strict framebuffer exclusion on physical test maps.
5. Run fresh all-board compile matrix and boot-to-shell on board profiles with multiple RAM intervals; measure recovered frame capacity and allocator scan latency, compare original single-region behavior.

## Success criteria
- [x] All admitted usable intervals contribute exactly their allocatable frames; no allocation spans a hole or enters reserved/firmware/MMIO pages. *(Reserved/MMIO frames are excluded by construction: only `Usable` entries become ranges, and the host fixture proves nothing is allocated in a gap between them.)*
- [x] Contiguous 4 MiB heap reservation still succeeds when an eligible interval exists and fails safely otherwise.
- [x] Memory telemetry is exact and framebuffer safety check tests cover lower/higher RAM intervals, not just maximum contiguous end. *(Telemetry sums over ranges and the framebuffer guard tests the whole range list for overlap; the guard's own syscall path is aarch64+RPi3-gated and has no host test — see Deviation Log.)*

## Progress

### Slice 1 — every usable range is managed (2026-09-27) — done

`FrameAllocator::new_from_map` kept only the largest `Usable` entry and derived
`memory_start..memory_end` from it: every other usable range was lost, `memory_end` was a
boundary that meant nothing outside that one region, and the framebuffer guard
(`base < allocator_end`) plus the boot memory log were computed from the same single region.

- `plan_managed_ranges` normalizes the map: page-align each entry, drop sub-page entries, sort,
  merge adjacent runs, and **halt** on overlapping entries or more than `MAX_MANAGED_RANGES`
  (8). A map that claims one frame twice cannot be resolved by guessing which claim wins.
- The allocator's index space is now the concatenation of the ranges (`index_base` per range),
  so a hole costs no bitmap bits and cannot be allocated. `frame_index_to_addr`,
  `addr_to_frame_index`, `allocate_contiguous` and `allocate_contiguous_aligned` are all
  range-aware, and a contiguous request is confined to one range — it never straddles a hole.
- `manages(addr)` answers "is this frame mine?"; the framebuffer guard uses it with a full
  overlap test instead of comparing against one end; `MemInfo`'s totals are the sum over ranges
  by construction; the boot log names every range and the total.
- The bitmap lives at the start of the largest range (which always has room for it) and its own
  frames are reserved. Bitmap storage is an enum so host fixtures own their bitmap instead of
  leaking one — the `Box::leak` in the test helper is gone.

- The planner and both contiguous allocators allocate **nothing**: they run before the heap
  exists. The first version collected the map into a `Vec` and took the RV64 boot down inside
  `new_from_map` (`h1-admission-lhg1gS` stops right after `kernel_phys_base`); the boot smoke
  caught it, and the rewritten version selects ranges in place.

Host tests (`cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu`, 118 passed):
every usable range contributes exactly its frames, adjacent entries merge, overlapping maps and
more than eight ranges halt, allocations never land in a hole, a 16-frame request across two
8-frame ranges fails rather than spanning the gap, and the boot-heap reservation still refuses
a fragmented map.

Boot smoke: RV64 test-hooks (`admission`, `asid-lease`), AArch64 test-hooks (the vfs-test
suite) and the RV64 production `launch-profile` lane all boot with the new allocator, each
logging its range inventory.

## Assumptions / risk / rollback
- [UNVERIFIED] Boot information for all boards distinguishes reserved intervals consistently; inspect DTB/Limine map generation and exact board fallback before implementation. Rolling back allocator layout changes requires a **cold boot with snapshot restore disabled** and no saved in-memory pointer reuse; existing snapshots and MMIO writes cannot be repaired retroactively. A physical board map is needed to claim reclaimed capacity there; host/QEMU only prove algorithm behavior.

## Deviation Log

- **Recovered-capacity claim is host-side only.** QEMU virt and the supported boards report a
  single usable range, so "recovered frames on a physical board" has no witness here. What is
  proved is the algorithm: every admitted range contributes its frames and nothing is allocated
  in a hole. The plan's own gate ("a physical board map is needed to claim reclaimed capacity
  there") stays closed.
- **`FALLBACK_MEMORY_MAP` not touched.** The plan's step 3 also requires the x86 fallback to
  prove it excludes the live kernel image (today it marks that range usable). That path is
  x86-only and its lane is not runnable here; it is recorded as open, not silently fixed.
- **Snapshot still uses `memory_start()/memory_end()`** — correct for a single range, wrong for
  a map with a hole. Snapshot capture/restore is disabled by phase 01 and phase 07 owns the
  range-aware format, so the accessors now document that callers must iterate
  `managed_ranges()` instead.
- **Subagent delegation remains unavailable** (Codex provider quota), so this slice's review was
  a session self-review.
