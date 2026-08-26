---
phase: 1
title: "Build the RV64 DTB Memory Map"
status: completed
priority: P1
effort: 1d
dependencies: []
tier: thinking
---

# Phase 1: Build the RV64 DTB Memory Map

> Log every Decision / Deviation / Surprise in the Deviation Log when it occurs.

## Overview

Replace the direct-OpenSBI RV64 static RAM ceiling with a bounded DTB-derived map that excludes
firmware, the live kernel, and all DTB reservations before publishing any `Usable` range.

## Requirements

- Resolve the effective DTB once: Limine response when present, otherwise the entry argument.
- Read every enabled RV64 memory-node range without calling panic-prone `Fdt::memory()`.
- Protect OpenSBI/firmware, `[kernel_base, align_up(__stack_top))`, `/memreserve/`, and enabled static `/reserved-memory` children.
- Reject malformed ranges, overflow, kernel-outside-RAM, dynamic reservations, no usable output, and bounded-buffer exhaustion.
- On rejection, log one reason and use the existing board-specific static fallback without truncation.
- Preserve non-overlapping normalized entries for paging; no allocator or paging semantics change.

## Architecture

`effective DTB -> allocation-free range builder -> sorted/merged protected intervals -> subtraction -> validated MemoryMapEntry slice -> SimpleBootInfo`.
The frame allocator continues choosing the largest `Usable` interval.

## Assumptions

- **Claim:** Current DTB consumers retain no borrowed FDT slices after early boot.
  **Confidence:** high
  **How to verify:** inspect `cpu_features::detect`, `platform::init`, and the new map builder before publishing DTB frames as usable.
- **Claim:** A 64-entry output is sufficient for supported firmware maps.
  **Confidence:** medium
  **How to verify:** fixture-test the maximum split count; overflow must select fallback rather than truncate.

## Related Files and Ownership

| File | Action | Owner |
|---|---|---|
| `kernel/src/boot.rs` | Modify: wire effective DTB and publish RV64 runtime boot info | Phase 1 only |
| `kernel/src/boot/dtb_memory.rs` | Create: pure bounded interval builder | Phase 1 only |
| `kernel/src/main.rs` | Modify: resolve/pass one DTB pointer and log chosen allocator range | Phase 1 only |
| `kernel/src/platform.rs` | Modify only if needed to remove duplicate DTB selection | Phase 1 only |
| `tests/boot-unit/Cargo.toml` | Modify: host DTB fixture dependencies | Phase 1 only |
| `tests/boot-unit/src/main.rs` | Modify: execute production range logic with fixtures | Phase 1 only |

## Implementation Steps

1. Extract an allocation-free map builder with typed failures and checked half-open interval helpers.
2. Collect all RAM ranges and protected ranges, sort/merge protection, then subtract into a fixed output buffer.
3. Size the RV64 kernel interval from the configured kernel base through aligned `__stack_top`.
4. Publish the generated map once during single-hart early boot; keep QEMU/VF2/Pioneer static maps as emergency fallback.
5. Resolve the DTB once in `kmain` and pass the same pointer to feature, platform, and memory discovery.
6. Add a stable boot marker containing allocator start, end, and managed byte count.
7. Add fixtures for 2 GiB RAM, multiple ranges, reservations, disabled nodes, overflow, malformed DTB, dynamic reservations, and kernel-outside-RAM.

## Test Matrix

| Scenario | Expected |
|---|---|
| QEMU 2 GiB, no extra reservation | More than 1 GiB remains usable after kernel/firmware subtraction |
| Header and static reserved ranges | No emitted `Usable` entry overlaps either range |
| Disabled reserved child | Child is ignored |
| Dynamic reserved child | Typed failure and audited static fallback |
| Overflow/malformed/zero RAM | Typed failure, no partial map, static fallback |
| VF2 and Pioneer compile | Board-specific kernel base and fallback remain valid |

## Success Criteria

- [x] Host boot-unit fixtures pass with production range code, including all failure paths.
- [x] RV64 default, `board-vf2`, and `board-pioneer` compile checks pass.
- [x] Every generated map is sorted, non-overlapping, and contains at least one usable interval.
- [x] A failed DTB map emits a reason and never silently returns a partial map.

## Security Considerations

Treat firmware ranges as untrusted arithmetic input. Never expose OpenSBI, kernel, DTB reservations,
or enabled reserved-memory frames to the allocator.

## Risk Notes and Rollback

Primary risk is misclassifying protected RAM as usable. Roll back the phase as one focused commit;
the audited static maps remain intact and are the runtime fallback. The only non-reversible effect is
runtime data corruption if overlap testing is skipped, so fixture gates must run before QEMU.

## Deviation Log

- **Surprise:** `__stack_top` was not the end of the RV64 kernel image. The linker orphaned
  `.got` immediately after it, so the first DTB build exposed `.got` as usable RAM and the
  allocator bitmap overwrote it at `0x82afa000`; full boot then faulted at scheduler start.
  `kernel/linker.ld` now places `.got` explicitly and publishes `__kernel_end` after it.
