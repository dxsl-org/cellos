# A1 runtime evidence — RV64 DTB memory map

**Baseline:** `976a6ac2` plus the uncommitted A1 working-tree changes described below.  
**Date:** 2026-07-31

## Verdict

A1 is implemented and passes the focused host, compile, capacity, and boot gates. The RV64
direct-OpenSBI lane now derives RAM from the firmware DTB, excludes firmware, the live kernel,
`/memreserve/`, and enabled static `/reserved-memory`, and falls back to the audited static map
on malformed or unsupported input.

The fresh full serial `boot` suite did not produce a final pass count: the harness exceeded its
20-minute timeout near test 28. Focused runtime paths passed after fixing the linker ownership
bug that the first full run exposed. The prior branch gate remains 54/54, but it is not presented
as a fresh post-A1 full-suite verdict.

## Implementation evidence

- Production parser/range logic: `kernel/src/boot/dtb_memory.rs` and
  `kernel/src/boot/dtb_memory_ranges.rs`.
- Early-boot wiring: `kernel/src/boot.rs`, `kernel/src/main.rs`, and `kernel/src/platform.rs`.
- Linker ownership fix: `.got` is explicit and `__kernel_end` follows it in `kernel/linker.ld`.
- Host fixtures execute the production range logic through `tests/boot-unit`.
- The integration runner accepts an RV64 memory size and the handoff test asserts more than
  1 GiB is managed under QEMU `-m 2G`.

## Verification

Passed:

- Boot-unit DTB fixtures: **15/15**, including rejection of `disabled`, `fail`, `reserved`,
  and malformed memory-node status values.
- `vicell-kernel` compile for default RV64, `board-vf2`, and `board-pioneer`.
- 2 GiB runtime capacity gate.
- Normal RV64 shell boot.
- Focused FAT, GPU, DHCP, echo, and concurrent IPC-pending runtime tests.
- `git diff --check`.

The decisive 2 GiB marker was:

```text
[boot] allocator range 0x82afb000..0x100000000 (2102415360 bytes)
```

That is greater than 1 GiB and cannot be produced by the former 190 MiB fallback.

The independent final rerun rebuilt against the shared dirty tree and reported
`2,102,411,264` bytes, one page less because concurrent kernel edits moved `__kernel_end` by one
page. Both runs prove the capacity criterion; the exact start address is image-size dependent.

Clippy remains blocked by the concurrent/pre-existing `match_like_matches_macro` warning in
`kernel/src/task.rs`; no A1 file introduced the warning.

## Artifact provenance

```text
RV64 kernel SHA-256: 7eddc2f6922c1e4a245e55784bd53c6d658ad6dca40915c1151b673c4e9c05e7
RV64 disk SHA-256:   de0e164a3bbfce6f3e5a0ab0118ae0d23c407e8e0f6a69eaa0736d1732f526b0
```

## Failure found during verification

The initial map used `__stack_top` as the kernel image end. The linker orphaned `.got`
immediately afterward, so the allocator began at `.got` and overwrote it, causing a scheduler
fault. Explicit `.got` placement plus `__kernel_end` fixed the corruption; the map builder aligns
that symbol outward to a page boundary.

## Residual limits

- The allocator still selects only the largest usable interval; A1 preserves discontiguous map
  entries, but multi-region allocation remains separate work.
- Dynamic `/reserved-memory` reservations are deliberately rejected and select the static map.
- A fresh full serial boot-suite count remains desirable because the post-fix run timed out.
