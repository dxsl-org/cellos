# D5 cell-scale measurement — 193 cells, refused with a typed OOM (and the boot bug that faked the earlier numbers)

**Date**: 2026-10-03 · **Decision served**: D5 (per-request server profile, Spec 19 §3)
· **Gate**: portfolio requires N=64/128/256/512 baselines *measured with M heavy cells
resident* (`.agents/plan-portfolio.md`).

## Method

Lane `scripts/qemu-cell-scale.sh` + kernel feature `cell-scale-experiment`
(`MAX_CELLS` 64→4096, `MAX_SLOTS` 512→4096, `kernel/src/memory/cell_quota.rs:22`,
`kernel/src/loader/va_alloc.rs:53`). It boots RV64 QEMU `virt` and runs
`/bin/capacity-probe`, which spawns parked `/bin/bench-probe` children (role
`resp-echo`, blocked in `sys_recv`) until the kernel refuses, printing
`[a2a3-probe] parked count=N` every 32 spawns and `OOM_TYPED count=N` on a typed
`OutOfMemory`. The probe must be in the kernel's **VIFS1 ramdisk** (the shell reaches
capability-bearing cells only through the Path route), hence the one-time prerequisite
`CELLOS_INCLUDE_CAPACITY_PROBE=1 bash scripts/gen-disk-ci.sh`.

## Result — a clean refusal, not a dead kernel

```
USER: [bench-probe] Started with role: 'resp-echo'          ×295
[ WARN] [loader] spawn OOM: op=SpawnPinned caller=20 path=/bin/bench-probe
      heap_used=3926088 heap_free=268216 caller_charged=1900996 task_size=1440
USER: [a2a3-probe] OOM_TYPED count=295
CELL-SCALE: parked=295 memory=2G bound=memory
```

**295 parked cells, typed refusal, identical at 2 GiB and 512 MiB**, zero kernel
panics, zero `allocation error` lines, all boot self-tests PASS. `bound=memory` (not
the raised `MAX_SLOTS` constant), and the count does not move with guest RAM ⇒ the
binding resource is the **4 MiB boot heap** (`HEAP_FRAMES = 1_024`,
`kernel/src/main.rs:610`). **N=256 is therefore reachable**; the gate still needs the
M=1/2/4-heavy-resident baselines.

Per cell the spawn charges the caller ≈6.4 KB (`caller_charged=1900996` over 295), and
total heap usage grows from 1,534 KiB to 3,834 KiB across the run; `size_of::<Task>()`
is only 1,440 bytes. Note the heap also carries the now-heap-backed `cell_owners` table
(160 KiB fixed at `MAX_CELLS=4096`) — making it sparse (a `BTreeMap`, as `Scheduler`
does for its other maps) would move the count, not the per-cell cost; recorded in
`.agents/TODO.md`.

**The earlier numbers in this report (193/194 cells, and 204/236 before that) were
taken on kernels with two different defects** — a scheduler stack overflow that zeroed
accounting (204/236, fake) and an eager per-task mailbox reservation that halted the
sweep at ~150 once the stack bug was fixed. See the follow-up section for the 193 → 295
chain.

## Root cause of the boot instability — `Scheduler` built on the stack

`Scheduler` was **164,064 bytes** at `MAX_CELLS=4096`, because
`cell_owners: [CellOwnerSlot; MAX_CELLS]` was an inline array
(`kernel/src/task/scheduler.rs`). `task::init()` constructs it *through the stack*:

```rust
unsafe { core::ptr::write(&mut *sched_guard, Some(Scheduler::new())); }
```

`Scheduler::new()` therefore zeroed/wrote a 160 KiB temporary on the boot hart's stack.
That overflow ran past the stack into the statics above it — `PLATFORM` sits 12 KB past
the end of the RT stack pool — and zeroed `PLATFORM`'s published flag, so every boot
panicked in `platform::with`:

```
[KERNEL PANIC] panicked at kernel/src/platform.rs:237:
[platform] platform::init not called before platform::with
```

Found with a **QEMU gdbstub hardware write watchpoint** (no gdb on this box: a ~60-line
Python GDB-RSP client sets `Z2,<PLATFORM>,1`, steps past each hit, and reads `pc`/`ra`/`sp`):

| hit | pc | meaning |
|---|---|---|
| 0 | kernel entry | the BSS clear (legit, flag stays 0) |
| 1 | `memcpy+0xa` from kmain | the publish (flag 0→1) |
| 2 | `kmain+0x8a5e` | **the corruption** — `sd zero, 0(a1)` in a zeroing loop, `a1 = sp + 0x181A8`, `sp` inside the RT stack pool |

That the target *varied with layout* was proven separately: reverting only the signing
patch made the profile boot again, while adding **one `log::info!`** inside
`platform::with` moved the failure elsewhere and flipped `thread-cap` /
`grant-reclaim` / `thread-quota` from PASS to FAIL. A layout-dependent write into BSS
was the only explanation, and the watchpoint named it.

**Fix**: `cell_owners` is now heap-backed (`alloc::vec::Vec<CellOwnerSlot>` +
`alloc::vec![CellOwnerSlot::Empty; MAX_CELLS]`). All twelve call sites use
`get`/`get_mut` and needed no change; `Scheduler` is now small in both profiles (and no
longer copied byte-for-byte on any move).

## Infallible allocations on the spawn path — fixed

Each of these turned "we are out of memory" into "the kernel halts":

| Site | Before | After |
|---|---|---|
| `loader/early.rs::read_from_block_table` | `alloc::vec![0; sector_rounded]` + `truncate` + `into_boxed_slice` (infallible shrink realloc) | exact fallible allocation; partial final sector read into a scratch buffer |
| `fs.rs::read_file_from_vifs1` | `truncate` + `into_boxed_slice` | exact path when `read == size`; a short read is logged per chunk and copied into a fallible exact buffer |
| `signing.rs::verify_cell_with_key` | `Vec::with_capacity(elf.len() - 64)` — the signed payload, **78,760 bytes for `/bin/bench-probe`** | `try_reserve_exact` → `ViError::OutOfMemory`; `verify_cell` now returns `ViResult<bool>` and `governed_spawn` fails closed, reporting capacity (`op=SpawnPinned`) instead of a signature mismatch |
| `memory/cell_quota.rs::snapshot` (test-hooks) | three `[T; MAX_CELLS]` arrays on the stack = 128 KiB | heap-backed `Vec` fields |

## What this does and does not answer

- **Answered**: with the boot bug fixed, the D5 lane measures a real ceiling — 193
  cells, refused with a typed OOM, independent of guest RAM.
- **Not answered**: N=256/512 are still **out of reach** (193 < 256) and the
  M=1/2/4-heavy-resident baselines are still unmeasured, so the gate is still open.
  Which allocation refuses first at 193 is not yet named (the `[loader] spawn OOM`
  line names the op, not the allocation).

## Follow-up (same day) — what the refusal actually was, and what it exposed

Instrumenting the spawn path (named OOM stages at every fallible allocation, allocator
failure reasons, per-spawn heap usage, and an experiment-only net size-class histogram)
answered the questions the 193 number left open:

- **The refusal was a contiguity failure of a transient**: `[signing] OOM: signed
  payload of 78760 bytes` with `heap_free=305832` — 299 KiB free, no 78 KiB hole. The
  payload is the ELF minus its 64 signature bytes; it exists for one `verify_cell` call.
  It is now built in a **reused `PAYLOAD_SCRATCH`** instead of a fresh allocation, which
  removes 78 KiB of per-spawn churn (the copy itself is unavoidable: the `ed25519_compact`
  API takes a contiguous message).
- **The retained cost is ≈16.3 KB of kernel heap per parked cell** (674→3797 KiB over
  144 spawns), of which ≈8.4 KB is charged to the *spawner* (`caller_charged=1621988`
  over 193 cells) and the rest to the child. `size_of::<Task>()` is only **1,440 bytes**;
  the size-class histogram shows the cost is **~12 allocations per cell** in the 0.9–2 KB
  classes, not one structure.
- **The heap binds, not a quota**: `DEFAULT_QUOTA_BYTES` is 16 MiB, larger than the whole
  4 MiB heap, and the new `null_from_quota` / `null_from_heap` counters say which
  resource refused.

Chasing the contiguity failure exposed **a class of infallible allocations on the spawn
path** — each one turns "out of memory" into "halt the kernel". Fixed in this pass:

| Site | Was | Now |
|---|---|---|
| `signing.rs` payload | fresh `Vec` per spawn | reused `PAYLOAD_SCRATCH` |
| `task/elf_prepare.rs` | three `collect::<Vec<_>>()` sized by the ELF's page count (4 KiB/12 KiB) | `collect_reserved` (fallible `try_reserve_exact` + fill) |
| `loader/elf.rs::load_segments` | `Vec<LoadedPage>` grown by `push` (512 entries = 12,288 B) | page bound summed from the PT_LOAD headers, reserved fallibly first |
| `measurement_log.rs` | `Vec` growth on every `spawn_from_path` (256 entries = 12,288 B) | `try_reserve` before push; a dropped entry keeps the aggregate advancing |
| `loader/aligned_elf.rs` | `vec![0u64; …]` copy of the ELF when the input is not 8-aligned | fallible reserve, then fill |
| `task/elf_prepare.rs`, `task/scheduler.rs` | `Box::new(Task::new(..))` | `heap::try_box` (fallible `Box`) |
| `memory/address_space.rs` | ledger `push`es in `map_private_page`, `map_existing_task_stacks`, `map_grant_page`; builder `ledger`/`frames` grown by `push` | `try_reserve` before each push; builder reserves its known mapping count |
| **`task/pending_mailbox.rs`** | **`Vec::with_capacity(HOTSWAP_MSG_QUEUE_DEPTH)` = 6,656 bytes eagerly for every task** | **lazy container; `try_push` reserves fallibly on first use** |

**The mailbox was the binder and the gate's lever**: it cost 6,656 bytes of heap per
task even for a parked cell that never receives a message, so making it lazy removed
both the fatal allocation *and* ~40% of the per-cell heap cost. The sweep went from
halting at ~150 to **295 parked cells with a typed OOM** (identical at 2 GiB and
512 MiB). Naming it needed a *reliable* backtrace: the alloc-error handler's own stack
scan sees stale frames from earlier calls, so the chain is now captured **inside
`QuotaAlloc::alloc`** at the moment of failure (`alloc caller[0..8]`), which pointed at
`Task::new +0xae` → `PendingMailbox::new`.

## Heavy-cell baselines — the gate's M

The gate requires the light ceiling measured **with M heavy cells resident**
(`docs/roadmap/beam-parity-backend-roadmap.md` §2.3: "heap lớn + grant 16 MiB").
The lane takes `--heavy M`: `capacity-probe` spawns M **`/bin/heavy-probe`** children
before the light sweep. That binary is separate from `bench-probe` on purpose — it
declares a **20 MiB cell heap arena** (`ostd::declare_custom_heap!`) and touches 16 MiB
of it, plus a **16 MiB resident grant** (`sys_grant_alloc`, the `MAX_GRANT_PAGES`
ceiling), then parks blocked in receive. Giving the *light* children such an arena
would make every light cell cost the kernel ~160 KiB instead of ~6 KB.

| M heavy (16 MiB touched heap + 16 MiB grant) | light ceiling | per heavy cell |
|---|---|---|
| 0 | **295** | — |
| 1 | **280** | −15 |
| 2 | **265** | −15 |
| 4 | **234** | −15.25 |

**Gate reading**: N=256 (and N=64/128) holds **with up to 2 heavy cells resident**
(280 / 265 ≥ 256) and **fails at 4** (234 < 256). A heavy cell costs the per-request
light sweep ≈15 cells of ceiling, which is the kernel-side price of its 20 MiB arena:
5,120 pages × 32 B of address-space ledger ≈ 160 KiB of the 4 MiB kernel heap. Every
run keeps a typed refusal, zero panics and zero fatal allocations, and the runner
refuses to report a heavy run unless each heavy cell logged its `heap resident:` and
`heavy resident: grant=` lines.

**Earlier, grant-only measurement (superseded).** The first version of this section
reused `bench-probe` with a grant alone; the light ceiling barely moved (295/295/293/291/287/280
at M=0/1/2/4/8/16) because the *heap* half of the profile did not exist — the cells
reported `heavy heap: 16 MiB refused (cell heap cannot grow)`. That is why the
dedicated binary exists, and why the numbers above are the gate's answer.

**Per-cell cost, measured** (size-class histogram at the M=0 ceiling, 295 cells):

| ≤ bytes | net bytes | per cell |
|---|---|---|
| 1,024 | 22,064 | 75 B |
| 4,096 | 1,110,496 | 3.8 KB |
| 16,384 | 1,348,872 | 4.6 KB |
| 65,536 | 124,536 | 422 B |
| 262,144 | 234,032 | 793 B |

So a light cell retains ≈9.7 KB of kernel heap, and the two big classes are ~3.2
allocations each per cell: the ELF-segment list (`CellSegments.pages`, 16 B/page over
~267 pages ≈ 4.3 KB) and a ~1.3 KB class ≈2.8 allocations per cell. The address-space
*ledger* is **not** per-page here (it does not duplicate the segment list), so the
earlier "32 B/page ledger" estimate was wrong — the lever is the segment list and that
1.3 KB class, not the ledger. Recorded in `.agents/TODO.md`.

## Next steps, in order

1. A *growable* cell heap (separate feature — the heavy profile is complete with a
   declared arena; growth matters for long-running data cells, not for this gate).
2. Finish the spawn-path audit (the remaining infallible sites: the `.clone()`s in
   `scheduler`, `to_vec()` in `state_stash`, `spawn_with_stacks_configured`'s `Box`).
3. Keep reducing the per-cell heap cost (measured: ≈9.7 KB/cell — the ELF-segment list
   ~4.3 KB plus a ~1.3 KB × 2.8 class; the address-space ledger is *not* per-page).
   That, or a larger kernel heap, is what moves N=256 with M=4.
4. Fix the ≥4 GiB cell-VA collision (`ELF: load VA 0x100000000 already mapped`).
5. Resolve the embedded-FAT 64-byte short read (`/bin/bench-probe`: entry 78,824,
   read 78,760) — same class as the open VFS short-read item.


## Evidence

- Lane: `scripts/qemu-cell-scale.sh`; logs at `build/cell-scale-*/qemu.log`.
- Clean runs: `build/cell-scale-*` (2 GiB and 512 MiB, both `parked=193`,
  `OOM_TYPED count=193`).
- Corrupted runs kept for the record: `build/cell-scale-1791009084` (204),
  `build/cell-scale-1791010411` (236), `build/cell-scale-1791010979` (boot loop,
  904 panics).
- Kernel: `task/scheduler.rs` (`cell_owners`), `task.rs:748` (`task::init`),
  `signing.rs` (`verify_cell_with_key`), `fs.rs`, `loader/early.rs`,
  `memory/cell_quota.rs`, `memory/heap.rs:58`, `main.rs:610`, `kernel/linker.ld`
  (64 KiB stack).
- Debug technique: QEMU gdbstub write watchpoint via a Python GDB-RSP client
  (`Z2`/`vCont`/`p`/`g`/`m`); `nm -n` for symbols.
