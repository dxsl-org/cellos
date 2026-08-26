---
phase: 03
title: Migrate P2 cell ELFs into a VFS-served filesystem (/bin overlay)
tier: thinking
status: pending
depends_on: [01]
origin: red-team F1 (fatal)
---

# Phase 03 — Migrate non-bootstrap cell ELFs into a VFS-served FS

## Context links
- Plan: [plan.md](plan.md) · Scout: [scout-report.md](scout-report.md)
- Red-team F1 (fatal): the load-bearing premise "VFS can serve `/bin/foo` by path" is FALSE today.

## Overview
**Priority:** blocks Phase 04. Today `/bin` in VFS proxies to the embedded VIFS1 ramdisk only (`backend_bootfs.rs:1-8`, `manager.rs:34-52`); the disk cell store (P2 `CELL_TABLE`) has no VFS backend. Before init/shell can read a disk-resident cell's ELF via VFS, those ELFs must live in a filesystem VFS mounts, and `/bin` must resolve both the VIFS1 (bootstrap) and disk (apps) sets.

## Key insights
- **P2-only cells today** (`gen_disk.ps1:424-473`, absent from VIFS1 `:336-375`): fb-console, robot-demo, robot-dashboard, the entire **Hypha** stack (llm-gateway, hypha, tool-fs, tool-sys, tool-spawn), nc/curl/wget/httpd/mqtt, bench/bench-probe, posix-shim-test, input-test, all Zig cells. These are the cells that actually depend on the disk — the ones this whole plan exists to keep working.
- VIFS1 already holds the bootstrap + common set (init, shell, vfs, config, net, input, compositor, supervisor, platform, nvme, e1000, virtio-net, virtio-gpu, net-broker, lua, doom, coreutils, demo cells).
- `/bin` → `BootFsProxy` reads VIFS1 via `sys_open`/`sys_read_cap`. Keeping the `/bin/…` namespace (so no call sites change) means `/bin` must become a **union/overlay**: VIFS1 first, disk-FS fallback.

## Requirements
- **Functional:** every P2-only cell is readable by its `/bin/…` path through VFS. Existing VIFS1 cells still resolve. Per-ELF Ed25519 signatures (`__ViCell_sig`) survive the copy so the loader gate still verifies them.
- **Non-functional:** disk FS choice must be writable by `gen_disk.ps1` on the host and mountable by the VFS cell at boot.

## Architecture — DECIDED with user 2026-07-07: dedicated FAT cell-store (option B)
Rationale (analysis): **littlefs cannot be host-populated** (no `mklittlefs`; core lives inside service-vfs) — so littlefs is out. **FAT is fully host-tooled** (`mkfat32.py`/`mkfat32_inplace.py` already build VIFS1 packed with `/bin` cells) and a read-only cell store needs no journaling. A **dedicated** partition (not P1 `/mnt/sd`) keeps cell binaries isolated + read-only + untampered.

- **Cell-store partition:** a new FAT volume addressed by a constant base LBA in `api::disk` (like P5 RedoxFS — no MBR slot needed; MBR has only 4 primary entries, all used). Place it AFTER P5: `PART_CELLSTORE_BASE_LBA = 1_062_144` (= P5 base 931_072 + 131_072), size ~32 MB (65_536 sectors). Disk (~1.86M sectors) has ample room.
- **⚠️ FAT stack is hardwired to P1.** `block_stream.rs` uses `const FAT_PART_BASE_LBA = 2_048`; `BlockStream`/`CachedBlockStream`/`FatBackend` all read P1 only. Phase 03 MUST parameterize these by a `base_lba` field so a SECOND FAT volume (the cell-store) can be mounted at a different base. This is the core code change (touches `block_stream.rs` + `backend_fat.rs`).
- **`/bin` overlay:** new `BinOverlay` backend (implements `FsBackend`) wrapping `BootFsProxy` (VIFS1) + a `FatBackend` bound to the cell-store base. Read ops (`get_file_ptr`/`list`/`stat`/`file_size`/`read_to_vec`) try VIFS1 first, then the cell-store; `list` merges both; all mutating ops return `false` (read-only). Mount it at `/bin` (replaces the bare `BootFsProxy` mount in `manager.rs:47`). Keeps the `/bin/…` namespace so no spawn call site changes.
- **Retire the raw P2 `CELL_TABLE`** for non-bootstrap cells (Phase 06 removes the kernel early-loader block reader). Bootstrap cells stay in VIFS1 (Phase 01). During transition both coexist: raw P2 table still served by early loader until Phase 04 spawn cutover.
- **gen_disk:** format the cell-store FAT (extend `mkfat32_inplace.py` at the new base) and insert the signed P2-only cell ELFs (via `fat16_insert.py`/`mkfat32.py` pattern). Keep writing the raw P2 table until Phase 06.

## Related code files
- Modify: `gen_disk.ps1` (write P2 cell set into the disk FS instead of the raw CELL_TABLE; preserve signatures), `cells/services/vfs/src/manager.rs` (mount + `/bin` overlay), `cells/services/vfs/src/backend_bootfs.rs` (overlay fallback) or a new `backend_cellstore.rs`.
- Read-only ref: `cells/services/vfs/src/backend_fat.rs`, `lfs_disk.rs`, `scripts/sign-cell.py` (sig preservation).

## Implementation steps
1. Choose target FS + mount point (recommend `/bin` overlay backed by a littlefs cell-store).
2. `gen_disk.ps1`: format the store, write each P2-only cell ELF (signed) into it; keep VIFS1 set as-is; stop writing those cells to the raw CELL_TABLE.
3. VFS: mount the cell-store; make `/bin` an overlay (VIFS1 → disk-store fallback).
4. Add integration test: `vfs_read("/bin/<a-P2-only-cell>")` returns byte-exact signed ELF; VIFS1 cell still resolves.
5. Verify signature bytes intact end-to-end (loader would reject on mismatch in Phase 04).

## Todo
- [ ] Target FS + mount decided (ADR line)
- [ ] gen_disk.ps1 writes P2 set into disk FS, sigs preserved
- [ ] VFS mounts cell-store + `/bin` overlay
- [ ] Integration test: disk-cell + VIFS1-cell both resolve via `/bin`
- [ ] Signature round-trip verified

## Success criteria
- **Runtime evidence:** boot log shows the cell-store mounted; `vfs_read("/bin/hypha")` (a P2-only cell) returns its signed ELF byte-exact; a VIFS1 cell (`/bin/shell`) still resolves. This is the prerequisite that makes Phase 04 buildable.

## Risk assessment
- *Signature corruption on copy* — the copy must be byte-exact (raw ELF blob, not re-linked); test asserts sig verifies.
- *Overlay precedence bug* — VIFS1 must win for bootstrap names to avoid a stale disk copy shadowing a trusted-core cell; test both orders.
- *P4 littlefs capacity* — Hypha stack + demos are large; size the partition (check `PART_LFS_SECTORS`) and log utilization.

## Security considerations
- ELFs move storage medium but the trust gate is unchanged — the loader verifies `__ViCell_sig` over the bytes at spawn (Phase 04). Overlay precedence (VIFS1 first) prevents a disk-planted cell from shadowing a trusted bootstrap name.

## Next steps
Phase 04 can now have callers read `/bin/…` via VFS and spawn via `sys_spawn_from_elf`.
