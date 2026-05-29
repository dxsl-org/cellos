# Phase 13 — Complete VFS Service

**Effort:** 100h | **Priority:** P2 | **Status:** partial | **Blockers:** Phase 04, Phase 06

## Overview

Promote VFS from RamFS-only read mostly to a full read/write VFS Cell with FAT32 backing on the VirtIO block device, async I/O, directory operations, and disk quota tracking. After this phase, the shell can perform full file lifecycle operations (create, write, read, mkdir, rm, stat) end-to-end.

## Context Links

- `docs/09-vfs.md` — VFS architecture
- `libs/api/src/fs.rs` — `ViFileSystem` trait (extend)
- Phase 04 (VirtIO block working), Phase 06 (external ELF loading exists, so `/bin/` is real disk)
- Phase 07 (FileHandle IPC) — handles flow through here

## Key Insights

- FAT32 is the simplest filesystem with broad tool support and well-understood specs. Use it as primary backend for v1.0. RedoxFS (referenced as viFS1 in CLAUDE.md naming) is a stretch.
- Use the `fatfs` crate (no_std-compatible, MIT/Apache) to avoid hand-rolling. Pin version explicitly.
- Async file I/O in SAS context = owned buffers (Law 2). `read(handle, buf: Box<[u8]>) → (Box<[u8]>, usize)` — pass buffer in, get it back with bytes count.
- Directory operations need consistent locking: a single mutex on the VFS Cell's internal state is fine for v1.0; per-inode locking can come later.
- Disk quota: track bytes-on-disk per CellId; reject write when exceeded. Quota lives in the cell registry (Phase 07).

## Requirements

**Functional**
- VFS Cell exposes IPC API: `Open`, `Close`, `Read`, `Write`, `Seek`, `Mkdir`, `Rmdir`, `Unlink`, `Stat`, `ReadDir`
- Backing: FAT32 on VirtIO block, mounted at boot from a fixed partition
- `RamFS` still available, mounted at `/tmp` (volatile)
- Disk quota enforcement per Cell
- Shell can: `echo hello > /test.txt`, `cat /test.txt` → `hello`, `ls /`, `mkdir /foo`, `rm /test.txt`

**Non-functional**
- Throughput: ≥ 5 MB/s sequential read, ≥ 2 MB/s write in QEMU
- Per-op latency: open < 5ms, read 4KB < 2ms, write 4KB < 5ms
- Concurrent open by multiple cells doesn't deadlock
- Zero `unsafe` in cells/services/vfs

## Architecture

```
                  Cell (shell / app)
                       │
                       │ IPC Call (open, read, write, …)
                       ▼
              VFS Cell (cells/services/vfs)
                       │
              ┌────────┴────────┐
              ▼                 ▼
         RamFS @ /tmp       FatFS @ /
              │                 │
              │                 ▼
              │       VirtIO Block driver (kernel, from Phase 04)
              │                 │
              └─ in-memory       └─ /dev/vda
```

Internal state of VFS Cell:
- Mount table: `BTreeMap<&'static str, Box<dyn ViFileSystem>>`
- Per-handle table: `BTreeMap<CapId, (FsHandle, FilePos, Owner)>`
- Quota table: `BTreeMap<CellId, BytesUsed>`

## Related Code Files

**Modify:**
- `cells/services/vfs/src/main.rs` — full IPC handler (currently stub-grade)
- `libs/api/src/fs.rs` — add `mkdir`, `rmdir`, `unlink`, `stat`, `readdir` methods on `ViFileSystem`; add `Stat` struct (size, type, perms, mtime)
- `kernel/src/fs/fat.rs` — verify or wrap `fatfs` crate; expose `mount(block_dev) → ViFileSystem`
- `kernel/src/fs.rs` — wire mount points from boot
- `cells/drivers/disk/src/lib.rs` — eventually drives VirtIO block from user space (for v1.0, keep kernel driver authoritative; cell is a passthrough)
- `libs/ostd/src/fs.rs` — add `File::create`, `File::write_all`, `Dir::create`, `Dir::read_entries` convenience methods

**Create:**
- `cells/services/vfs/src/mount.rs` — mount/unmount logic
- `cells/services/vfs/src/quota.rs` — per-cell byte tracking
- `cells/services/vfs/src/handle_table.rs` — capability-keyed file-handle state
- `tests/integration/vfs_full.rs` — comprehensive lifecycle test
- `scripts/format-disk.sh` — formats disk image as FAT32, copies `/bin/`, `/etc/`, etc.
- `docs/vfs-api.md` — IPC message schema, error model

## Implementation Steps

1. **Dependency add**: `fatfs = { version = "0.4", default-features = false, features = ["alloc", "lfn"] }` in `cells/services/vfs/Cargo.toml`
2. **Wrap fatfs** as a `ViFileSystem`:
   - Adapter `FatFsAdapter { fs: fatfs::FileSystem<BlockDeviceProxy> }`
   - `BlockDeviceProxy` calls into kernel via IPC to access VirtIO block (read/write at LBA)
   - For v1.0, BlockDeviceProxy can be in-cell unsafe-free since IPC handles all unsafe at the kernel boundary
3. **Extend `ViFileSystem` trait** in `libs/api/src/fs.rs`:
   ```rust
   pub trait ViFileSystem: Send + Sync {
       async fn open(&self, path: &str, mode: OpenMode) -> Result<FsHandle, ViError>;
       async fn close(&self, h: FsHandle) -> Result<(), ViError>;
       async fn read(&self, h: FsHandle, pos: u64, buf: Box<[u8]>) -> Result<(Box<[u8]>, usize), ViError>;
       async fn write(&self, h: FsHandle, pos: u64, data: Box<[u8]>) -> Result<usize, ViError>;
       async fn stat(&self, path: &str) -> Result<Stat, ViError>;
       async fn mkdir(&self, path: &str) -> Result<(), ViError>;
       async fn rmdir(&self, path: &str) -> Result<(), ViError>;
       async fn unlink(&self, path: &str) -> Result<(), ViError>;
       async fn readdir(&self, path: &str) -> Result<Vec<DirEntry>, ViError>;
   }
   ```
4. **VFS Cell IPC dispatch** in `cells/services/vfs/src/main.rs`:
   - Listen on a fixed endpoint ID
   - Match incoming message variant → route to right fs based on mount table
   - Maintain `handle_table` for open files; map `CapId → (mount_path, fs_handle, pos)`
   - On open: allocate CapId via kernel cap registry, register in handle_table, reply with `ViFileHandle`
5. **Implement mount/unmount** `cells/services/vfs/src/mount.rs`:
   - `mount(point: &str, fs: Box<dyn ViFileSystem>)` — inserts into mount table
   - At boot: VFS Cell mounts FatFs (backing /dev/vda) at `/`, mounts RamFs at `/tmp`
6. **Implement quota** `cells/services/vfs/src/quota.rs`:
   - Hooks into write path: check before write, increment after success
   - Per-cell default 10 MB; configurable via Config Cell
   - On unlink: decrement quota
7. **OSTD wrapper expansions** `libs/ostd/src/fs.rs`:
   - `File::create(path) → Result<File>`
   - `File::write_all(&mut self, data: &[u8])` — chunks to owned buffers, calls IPC
   - `Dir::read_entries(&self) → Vec<DirEntry>`
8. **Format disk script** `scripts/format-disk.sh`:
   - `dd if=/dev/zero of=disk.img bs=1M count=64`
   - `parted disk.img mklabel msdos mkpart primary fat32 1MiB 100%`
   - `mformat -i disk.img@@1M -F`
   - Copy `/bin/{init,config,vfs,shell,…}` and `/etc/hostname` etc. via mcopy
9. **Integration test** `tests/integration/vfs_full.rs`:
   - Boot
   - shell: `mkdir /test && echo hi > /test/a.txt && ls /test && cat /test/a.txt && rm /test/a.txt && rmdir /test`
   - Assert serial output matches expected sequence
10. **Document** `docs/vfs-api.md` with message schema + error codes + quota semantics.

## Todo List

- [ ] Add `fatfs` dependency to vfs cell (deferred — FAT32 needs VirtIO stable)
- [ ] Implement `FatFsAdapter` wrapping the fatfs crate (deferred)
- [x] Extend `ViFileSystem` trait with readdir (mkdir/rmdir/stat already present)
- [x] Implement IPC dispatch: OP_MKDIR, OP_RMDIR, OP_UNLINK added to vfs/src/main.rs
- [x] Implement mount table (MountTable with "/" and "/tmp" entries)
- [x] Implement quota tracking (QuotaTracker with 32 MiB default per cell)
- [ ] Extend OSTD wrapper (File::create, Dir::read_entries — deferred to FAT32 phase)
- [x] Write `scripts/format-disk.ps1`; disk image regeneration documented
- [ ] Integration test `tests/integration/vfs_full.rs` (QEMU + FAT32 deferred)
- [x] Write `docs/vfs-api.md` (complete IPC protocol reference)
- [ ] Bench: ≥ 5 MB/s read, ≥ 2 MB/s write (requires VirtIO-FAT)
- [ ] CI green

## Success Criteria

- Full file lifecycle from shell works on FAT32
- /tmp on RamFs survives within a boot but not across reboots
- Read ≥ 5 MB/s, write ≥ 2 MB/s in QEMU
- Quota: cell exceeding 10 MB write gets `ViError::QuotaExceeded`
- `tests/integration/vfs_full.rs` green
- No `unsafe` in vfs cell

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| `fatfs` crate not perfectly no_std-friendly | Low | Med | Already supports no_std with `alloc`; pin version; provide minimal patch if needed |
| BlockDeviceProxy IPC overhead dominates throughput | High | Med | Batch in 64KB chunks; later kernel could grant DMA buffer caps to vfs cell |
| Quota race: two concurrent writes both pass the check | Med | Med | Serialize writes per-cell within VFS via per-cell mutex; revisit if perf forces |
| Mount-table reload after add breaks open handles | Low | Med | Lock mount table during op; document non-atomic mount as v1.0 limitation |
| Disk image format mismatch between QEMU and gen script | Med | Low | Single source of truth in `scripts/format-disk.sh`; CI regenerates fresh each run |

## Security Considerations

- Path traversal: resolve `..` and reject paths that escape the cell's allowed prefix (future: per-cell filesystem views)
- Symlink handling: not in v1.0 scope (FAT32 has no native symlinks; if added later, mind TOCTOU)
- Quota prevents one cell from filling disk (DoS); enforce per-cell + global cap
- File mtime/atime: rely on system time from Phase 22 timer infra; for v1.0 acceptable to set epoch = boot

## Rollback

VFS Cell PR is independent. Revert restores RamFs-only behavior; `/bin/` was loaded via early loader (Phase 06) which can stand on a raw TAR alternative if FAT32 falls through.

## Next Steps

Unblocks Phase 14 (input cell logs events to /var/log), Phase 17 (shell I/O redirection), Phase 18 (lua/micropython read scripts from disk), Phase 20 (hot migration loads new ELF from disk via VFS cap).
