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
- [ ] All admitted usable intervals contribute exactly their allocatable frames; no allocation spans a hole or enters reserved/firmware/MMIO pages.
- [ ] Contiguous 4 MiB heap reservation still succeeds when an eligible interval exists and fails safely otherwise.
- [ ] Memory telemetry is exact and framebuffer safety check tests cover lower/higher RAM intervals, not just maximum contiguous end.

## Assumptions / risk / rollback
- [UNVERIFIED] Boot information for all boards distinguishes reserved intervals consistently; inspect DTB/Limine map generation and exact board fallback before implementation. Rolling back allocator layout changes requires a **cold boot with snapshot restore disabled** and no saved in-memory pointer reuse; existing snapshots and MMIO writes cannot be repaired retroactively. A physical board map is needed to claim reclaimed capacity there; host/QEMU only prove algorithm behavior.

## Deviation Log
None.
