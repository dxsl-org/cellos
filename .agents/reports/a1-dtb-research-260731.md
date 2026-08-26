# A1 research — RISC-V DTB memory map

**Date:** 2026-07-31  
**Scope:** research only; no production code changed.

## Verdict

A1 is small enough to implement in the existing boot layer, but a direct replacement of the
190 MiB `Usable` entry with the DTB `/memory` range would corrupt OpenSBI or the kernel. The safe
shape is: read all RAM ranges from the firmware DTB, subtract every protected interval, then emit
non-overlapping `Bootloader`, `Kernel`, `Reserved`, and `Usable` entries. Keep the current static
maps only as a fail-closed emergency path when the DTB is absent, malformed, unsupported, or the
bounded output buffer overflows.

The minimum proof is a QEMU `virt -m 2G` integration test which observes more than 1 GiB in the
frame allocator. A boot-success test alone does not prove A1: the current 190 MiB map boots.

## Current boot and DTB path

- OpenSBI direct boot enters `_start` with `a0 = hart ID`, `a1 = DTB physical address`. The RV64
  assembly deliberately preserves both registers through relocation and BSS clearing before
  calling `kmain` (`hal/arch/riscv/src/rv64/boot.rs:65-103`).
- `kmain(hartid, dtb)` already passes the entry argument to CPU feature detection and platform
  discovery (`kernel/src/main.rs:92-100`). `platform::init` additionally prefers Limine's DTB
  response because under a Limine boot `a1` is not the firmware DTB
  (`kernel/src/platform.rs:103-114`, `kernel/src/boot/limine.rs:228-285`).
- The memory path is separate. `parse_bootloader_info()` first tries Limine; on direct OpenSBI it
  fails and `fallback_boot_info(dtb)` is selected (`kernel/src/main.rs:236-247`). For RV64 that
  function ignores `dtb` and returns the compile-time map (`kernel/src/boot.rs:232-286`,
  `kernel/src/boot.rs:452-458`). QEMU usable RAM is therefore always
  `0x8420_0000..0x9000_0000`, exactly 190 MiB.
- `FrameAllocator::new_from_map` selects only the largest `Usable` entry and assumes at least one
  valid range exists (`kernel/src/memory/frame.rs:49-67`). Paging maps every `Usable`, `Kernel`,
  and `Bootloader` entry and skips `Reserved` (`kernel/src/memory/paging.rs:101-152`). Therefore
  overlapping entries are not harmless: an overlapping `Usable` entry lets the allocator issue
  protected frames even if a second entry labels them `Kernel` or `Reserved`.
- AArch64 fallback code already reads `tree.memory().regions()` and sizes the kernel using
  `__stack_top` (`kernel/src/boot.rs:382-449`). It is a useful kernel-span pattern, not a complete
  A1 template: it reads one region, does not subtract reservations, and `Fdt::memory()` panics if
  the required node is absent.

## Available crates and APIs

The kernel already depends on `fdt = 0.1.5` for RV64 and AArch64 (`kernel/Cargo.toml:34-41`), so
no production dependency is needed.

Useful `fdt 0.1.5` APIs:

- `unsafe Fdt::from_ptr(ptr)` validates null, magic, and declared total size, then exposes the
  blob. The firmware pointer is still a trusted boot-contract pointer; an invalid mapped address
  can fault before validation.
- `find_node("/memory")` / `FdtNode::reg()` avoid the panic in `Fdt::memory()` and decode root
  `#address-cells` / `#size-cells` into `(starting_address, size)` tuples.
- `all_nodes()` permits multiple `memory@...` nodes and filtering disabled nodes.
- `memory_reservations()` exposes the FDT header reservation block (`/memreserve/`) as address
  and size.
- `find_node("/reserved-memory")`, then `children()` and `reg()`, exposes static reserved-memory
  child ranges.
- `total_size()` gives the DTB blob extent if it must remain reserved.

`vm-fdt = 0.3.0` is already a workspace dependency and is used by the hypervisor DTB builder.
It supports `FdtWriter::new_with_mem_reserv`, so it is the right host-test fixture builder; it
does not need to enter the kernel dependency graph.

## Reservation and range rules

Use half-open intervals `[start, end)` and checked arithmetic throughout.

1. Treat all DTB memory-node `reg` tuples as candidate RAM, not immediately usable memory.
2. Always protect the live firmware/kernel span independently of the DTB:
   - `[ram_start, kernel_base)` as `Bootloader` on the direct OpenSBI layout. This preserves the
     current 2 MiB OpenSBI exclusion without assuming that every firmware describes itself.
   - `[kernel_base, align_up(__stack_top, 4 KiB))` as `Kernel`. The current fixed 64 MiB RV64
     window is both wasteful for small images and unsafe for an embedded image larger than it.
3. Subtract every FDT header reservation (`memory_reservations()`). DTSpec reserves these ranges
   from normal client-program use.
4. Subtract every enabled `/reserved-memory` child with a static `reg`. Keep `reusable` regions
   reserved until Cellos implements an owner/reclaim protocol; honor `no-map` naturally by
   emitting `Reserved`, which paging already skips.
5. A `/reserved-memory` child with `size` but no `reg` requests dynamic placement. Do not silently
   ignore it. The smallest safe A1 behavior is to reject the DTB-derived map and use the audited
   fallback; allocating such regions is separate work.
6. Align usable ranges inward to 4 KiB; align protected ranges outward. Drop empty ranges.
7. Sort and merge protected intervals before subtraction. Emit a non-overlapping map only after
   validating that the live kernel is wholly contained in a DTB RAM range.
8. On checked-add failure, missing/zero-sized memory, unsupported cell sizes, no final usable
   range, or bounded-buffer exhaustion, log one reason and use the static fallback. Never truncate
   a generated map: truncation can drop the reservation that made an earlier usable range safe.

The DTB blob itself can be reclaimed after A1 parsing because current consumers copy all needed
data into kernel statics before frame allocation (`cpu_features`, `platform`, and boot info retain
no FDT references). If future code retains DTB-backed slices, reserve
`[dtb_ptr, dtb_ptr + total_size)` until that lifetime ends.

## Recommended architecture

Keep this in the boot layer; neither the allocator nor paging should learn DTB semantics.

1. Resolve the effective firmware DTB pointer once: Limine DTB response when present, otherwise
   the architecture entry argument. Pass that same value to CPU detection, platform discovery,
   and fallback boot-info construction. This removes the current three-consumer inconsistency.
2. Add a bounded, allocation-free RV64 map builder. Inputs should be the parsed FDT, runtime
   `kernel_base`, and runtime `kernel_end`; output is a static `MemoryMapEntry` slice or a typed
   error. Keep parsing/splitting separate from static publication so the range algorithm is
   host-testable.
3. Publish the generated slice through the existing `SimpleBootInfo`; retain the per-board static
   maps only for failure. `board-vf2` and `board-pioneer` then naturally use their firmware RAM
   descriptions instead of new RAM-size feature flags.
4. Log the chosen allocator range and byte count before moving the allocator into the global.
   This is an operational contract and gives the integration test a stable, machine-readable
   assertion.

`MAX_MEMORY_MAP_ENTRIES = 64` is probably sufficient, but generation must return overflow rather
than copy Limine's current truncate-and-warn behavior. One RAM range split by N reservations can
produce roughly `2N+1` pieces.

## Exact files

Production changes:

- `kernel/src/boot.rs` — effective-DTB helper, RV64 dynamic boot-info statics, runtime kernel-end
  symbol use, DTB map construction/publication, and static failure fallback.
- `kernel/src/main.rs` — resolve the DTB once, pass it to all consumers, and print the selected
  allocator range/size.
- `kernel/src/platform.rs` — small signature cleanup only if `main.rs` now passes the already
  resolved pointer; remove its duplicate Limine selection to keep one source of truth.

No change is required in `kernel/linker.ld`: `__stack_top` already bounds the loaded kernel plus
its boot stack (`kernel/linker.ld:55-61`). No allocator or paging change is required for A1 if the
boot map is non-overlapping and always contains a usable range.

Tests:

- `tests/boot-unit/Cargo.toml` — add host-only `fdt` and `vm-fdt` dependencies.
- `tests/boot-unit/src/main.rs` — replace the RV64 constant-only assertion with DTB fixtures for:
  2 GiB QEMU RAM; kernel/OpenSBI subtraction; header reservation; `/reserved-memory` reservation;
  multiple RAM tuples; disabled reserved node; overflow; checked-add failure; absent/invalid DTB;
  dynamic reserved-memory rejection; and kernel-outside-RAM rejection. Prefer including the pure
  range module by path over mirroring its logic, because this test crate currently duplicates
  production behavior (`tests/boot-unit/src/main.rs:1-18`).
- `tests/integration/src/lib.rs` — add a memory-size parameter (or `boot_rv64_with_memory`) instead
  of the hardcoded `-m 256M` at lines 303-322.
- `tests/integration/tests/handoff.rs` — add `handoff_rv64_uses_dtb_memory_size`: boot with 2 GiB,
  wait for the allocator-size marker, assert it exceeds 1 GiB, then wait for paging/heap. This
  catches both the old 190 MiB fallback and accidental reservation of almost all RAM.

## Verification gates

Run at minimum:

```text
cargo test --manifest-path tests/boot-unit/Cargo.toml --target x86_64-unknown-linux-gnu
cargo check -p vicell-kernel --target riscv64gc-unknown-none-elf
cargo check -p vicell-kernel --target riscv64gc-unknown-none-elf --features board-vf2
cargo check -p vicell-kernel --target riscv64gc-unknown-none-elf --features board-pioneer
cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu \
  --test handoff handoff_rv64_uses_dtb_memory_size -- --nocapture
```

Then boot the normal 256 MiB image and the existing boot suite. A1 is complete only when both the
2 GiB capacity assertion and the existing 256 MiB boot path pass; a static `qemu-virt-1g`-style
feature is not a DTB fix.

## Residual limits outside A1

- The allocator still manages only the largest usable interval, so discontiguous RAM above a
  reserved hole is not combined. A1 should preserve every interval in the map, but multi-region
  allocation is separate work.
- The `fdt` parser trusts firmware structure enough to contain internal `unwrap`/`expect` paths.
  Cellos already accepts this trust for platform discovery. Hardening against malicious firmware
  DTBs would require a stricter parser or pre-validation and should not block the QEMU/board RAM
  loss fix.
- RV32 retains its static map unless A1 scope is explicitly widened; its entry ABI is similar, but
  its bare-physical and 32-bit overflow constraints deserve separate tests.
