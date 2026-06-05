# Phase 29 — Heap Snapshotting / Instant On

**Status**: 📋 PLANNED  
**Priority**: P2  
**Target**: 2026-10-06  
**Effort**: ~3 weeks  
**Created**: 2026-06-05

---

## Goal

Save the kernel's physical memory state to disk after full boot, then restore it on subsequent boots — skipping ELF loading, heap initialization, BTreeMap rebuilding, and scheduler setup.

> Killer feature: sub-100ms warm boot. Không OS nào khác có.

---

## Scope Decision: Allocated-frames-only snapshot

**Research finding (2026-06-05):**

The spec says "4–8 MB for kernel + 6 base cells." Full 32 MB heap dump would take ~1 second on QEMU TCG (30 MB/s), violating the 100ms target. The frame allocator bitmap identifies exactly which frames are actually allocated — snapshot only those.

- **Snapshot scope**: ALLOCATED physical frames only (skip free frames). Typical running system: 4–8 MB.
- **Cells NOT individually snapshotted**: After warm boot, EarlyLoader respawns them from disk (already works). The cell heap/stack data IS snapshotted as part of allocated frames.
- **Performance reality on QEMU TCG**: 4–8 MB at ~30 MB/s = 133–266 ms. Sub-100ms requires real hardware (eMMC at 100+ MB/s).
- **Storage**: Raw LBA sectors at a RESERVED fixed offset in `disk_v3.img`. NOT FAT16 (FAT overhead would add 30–50% write amplification).

---

## Phases

| # | File | Status | Effort |
|---|------|--------|--------|
| 1 | [phase-01-serialization.md](phase-01-serialization.md) | 📋 PLANNED | 4 days |
| 2 | [phase-02-warm-boot-restore.md](phase-02-warm-boot-restore.md) | 📋 PLANNED | 4 days |
| 3 | [phase-03-invalidation-tests.md](phase-03-invalidation-tests.md) | 📋 PLANNED | 2 days |
| 4 | [phase-04-benchmark.md](phase-04-benchmark.md) | 📋 PLANNED | 1 day |

**Execution order**: 1 → 2 → 3 → 4.

---

## Current State (2026-06-05)

| Component | Status |
|-----------|--------|
| `docs/specs/03-runtime.md §4` | Spec exists: format, constraints, warm boot sequence |
| `kernel/src/memory/frame.rs` | Bitmap allocator — CAN enumerate all allocated frames |
| `kernel/src/task/drivers/virtio_blk.rs` | `write_sector(lba, buf)` exists for raw sector access |
| Snapshot code | **Does not exist** — starting from scratch |
| CRC library | Not added yet (`crc32fast`) |
| Kernel build hash | Not baked in yet (`build.rs` needed) |

---

## Snapshot Header Format

48-byte header, followed immediately by packed 4096-byte frames:

```
[magic:        u32 = 0x5649_4355]  // "VICU" LE
[version:      u16]                 // format version (increment on breaking changes)
[flags:        u16]                 // bit0=reserved, bit1=reserved
[kernel_hash:  u32]                 // CRC32 of kernel ELF (from build.rs env var)
[_pad:         u32]                 // alignment
[pa_base:      u64]                 // PA start of snapshotted region
[pa_end:       u64]                 // PA end (exclusive)
[frame_count:  u32]                 // number of 4096-byte frames (allocated only)
[heap_pa_start: u32]               // offset of heap start from pa_base
[crc32:        u32]                 // CRC32 of all bytes above (crc32 field = 0 during calc) + all frame data
[_pad:         u32]                 // align to 48 bytes
```

See `docs/specs/03-runtime.md §4` for the authoritative spec.

---

## Key Design Decisions

### CRC32 (not BLAKE3)
`crc32fast v1` with `default-features = false` — no_std, no_alloc, fast. Sufficient for accidental-corruption threat model. BLAKE3 is cryptographic overkill for this use case.

### Kernel hash via build.rs
`build.rs` emits `KERNEL_ELF_HASH=<crc32_of_binary>` via `cargo:rustc-env`. Kernel reads it via `env!()` at compile time — zero runtime cost.

### Raw sector storage
Reserved LBA range `SNAPSHOT_BASE_LBA = 200_000` in `disk_v3.img` (beyond the cell table at LBA 82000). No FAT overhead. Direct `viVirtIOBlk.write_sector()` calls.

### Restore checkpoint in boot sequence
Between `task::drivers::init()` (Step 12 in main.rs) and `EarlyLoader::probe()` (Step 13) — after VirtIO is initialized (needed for disk reads) but before cells are spawned (they get skipped on warm boot).

### VirtIO reinit after restore
Call `task::drivers::init()` AGAIN after restoring snapshot frames. This overwrites the restored `BLOCK_DEVICE` global — that's correct. Hardware state cannot be snapshotted; device registers reset each boot.

---

## Success Criteria

- [ ] `snapshot` shell command writes header + allocated frames to raw LBAs
- [ ] Warm boot detects valid snapshot and restores kernel heap in place of cold init
- [ ] Kernel log shows `[snapshot] warm boot: restored N frames in Xms`
- [ ] Stale snapshot (kernel rebuilt) → cold boot automatically
- [ ] Corrupted snapshot (wrong CRC32) → cold boot automatically
- [ ] All 65 existing integration tests pass on warm boot
- [ ] Warm boot time measured and documented (QEMU TCG vs. target performance)
