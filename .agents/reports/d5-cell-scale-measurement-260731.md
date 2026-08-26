# D5 — how many cells actually fit, measured

**Date**: 2026-07-31 · **Decision it serves**: D5, per-request server profile committed
2026-07-31 (Spec 19 §3). Measure before designing, so the image-sharing win is sized rather
than assumed.

## Method

Spawn parked cells (`/bin/bench-probe` in the `resp-echo` role, which blocks in `sys_recv`
forever) until the kernel refuses, reporting count and per-spawn cost every 32 spawns.
Parked rather than exiting — an exiting cell frees its frames so nothing accumulates — and
parked rather than spinning, so the latency figures are not polluted by CPU contention.

Experiment build (worktree, **not** committed): `MAX_CELLS` raised 64 → 512
(`kernel/src/memory/cell_quota.rs:15`; the constant only sizes two static arrays, so this
costs 8 KB of `.bss`). Other ceilings checked first: the scheduler's task table is a
`BTreeMap` (no bound); `MAX_SLOTS = 512` in `kernel/src/loader/va_alloc.rs:48` is the real
cap on PIE cells and bounds the experiment at 512.

QEMU `virt`, 2 GiB guest RAM, RV64, so that memory rather than a constant is the binding
limit. `bench-probe` is 67 136 bytes — a realistic light handler, not a toy.

## Result 1 — refusal at **n = 8**, after the suite had run

```
[scale] STOP at n=8 — spawn refused (Err(Unknown))
[scale] total parked cells = 8
```

**`Err(Unknown)` is `OutOfMemory`.** The handler maps it that way:
`types::ViError::OutOfMemory => SyscallError::Unknown`
(`kernel/src/task/syscall.rs:2579`). So the ceiling is memory, not a policy limit — and the
kernel logged *nothing* while refusing.

Eight. With 2 GiB of guest RAM, `MAX_CELLS` at 512 and 512 free VA slots.

## Finding A — the error mapping hides the diagnosis

`OutOfMemory → Unknown` means the one failure a capacity experiment most needs to see is the
one the ABI cannot express, and no log line accompanies it. Anyone hitting this in production
sees `Unknown` from `sys_spawn_pinned` and has nothing to go on. This is worth fixing
independently of D5: map it to a distinct error and log the allocation that failed. Compare
`ViError::NotFound → FileNotFound`, which *is* faithfully mapped two lines above.

## Result 2 — **n = 9** on unfragmented memory: not fragmentation

Moving the scale loop to run *before* the rest of the suite (so no cell has yet churned
memory) moved the ceiling from 8 to 9. Fragmentation is therefore not the cause, and the
`65`-contiguous-frame stack requirement is not what binds — which rules out the phase-08
follow-up (non-contiguous stack VA) as the first fix.

## Finding B — the real ceiling is **190 MiB of hardcoded RAM**, and it is not per-cell cost

The frame allocator takes the largest usable region from the boot memory map
(`FrameAllocator::new_from_map`, `kernel/src/main.rs:272`) and scans the whole bitmap for a
contiguous run, so neither is a limit. But on RISC-V the map itself is a **compile-time
constant**:

```rust
// kernel/src/boot.rs:232-250 — "RISC-V QEMU virt (256 MB at 0x8000_0000)"
MemoryMapEntry { base: 0x8420_0000, length: 0x0BE0_0000, ty: MemoryType::Usable }
```

`0x0BE0_0000` = **190 MiB**. The guest was given 2 GiB and the kernel never looked: there is
no DTB memory-node parse on this path, only `FALLBACK_MEMORY_MAP`. That 190 MiB carries the
kernel heap, the ~14 cells init spawns at boot (each 512 KiB of stack plus a full ELF copy),
and their heaps. What remains fits exactly nine more cells.

**So the binding constraint is neither of the two costs Spec 19 §3 set out to fix.** Three
ceilings stack up and the smallest is the one nobody was discussing:

| Ceiling | Value | Binding? |
|---|---|---|
| `MAX_CELLS` | 64 (raised to 512 for this run) | No |
| `MAX_SLOTS` (VA slots) | 512 | No |
| **RAM the kernel can see** | **190 MiB, hardcoded** | **Yes** |
| Per-cell cost | 512 KiB stack + full ELF copy | Second |

Consequence for the roadmap: reading the real memory map is *far* cheaper than image sharing
or demand-paged stacks, and at today's per-cell cost a visible 2 GiB alone reaches roughly
100 cells with no loader change at all. It also means every deployment silently discards all
RAM above 190 MiB — this is not a benchmark artefact.

## Revised order of work for the per-request server profile

1. **Parse the DTB memory node** on RISC-V instead of falling back to the hardcoded map.
   Cheapest, largest lever, and currently loses all RAM above 190 MiB on every machine.
2. **Share `.text`/`.rodata` across instances of one image** — safe now that Layer A has made
   those segments read-only.
3. **Demand-paged stacks** — removes the 512 KiB pre-allocation.
4. **Raise `MAX_CELLS` / `MAX_SLOTS`** last, once 1–3 have changed the denominator.

Spec 19 §3 lists 2 and 3 but not 1, and had the priority wrong; it should be amended to put
the memory map first.

## What Result 1 alone already settled

The per-request server profile cannot be reached by raising constants: `MAX_CELLS = 512` and
512 free VA slots were both in place and the system stopped at 8. That was the D5 reopening's
claim, now with a number.

Corollary for Spec 21: no document should carry a cell-count claim in prose. Today the number
is 9 under a real workload, 64 by constant, 512 by VA slots, 190 MiB ÷ per-cell cost by
physics, and 1000+ by aspiration. Only a generated figure can be true.

## What is already decided by Result 1 alone

Whatever Result 2 shows, the per-request server profile cannot be reached by raising
constants. `MAX_CELLS = 512` and 512 VA slots are both in place in this build and the system
stopped at 8, three orders of magnitude below the 1000+ target. The blockers are the
allocation policies named in Spec 19 §3, not the bounds — which is what the D5 reopening
argued, now with a number attached.

Corollary for Spec 21: `docs/system-architecture.md` and the PDR should not carry a cell-count
claim in prose at all. The number is 8 today under a real workload, 64 by constant, 512 by VA
slots, and 1000+ by aspiration; only a generated figure can be true.

## Also noted

`cells/tests/bench/src/scenarios/memory_footprint.rs:53-55` does not measure anything — it
returns `APPROX_BOOT_BYTES`, a compile-time constant, with `// TODO: replace with MemInfo
syscall when implemented`. The suite reports it as PASS. There is no MemInfo syscall and no
free-frame accounting anywhere in `kernel/src/memory/frame.rs` (only `total_frames`), which is
why this experiment had to infer capacity by spawning until refusal instead of reading it.
A MemInfo syscall would make every future capacity question directly measurable.
