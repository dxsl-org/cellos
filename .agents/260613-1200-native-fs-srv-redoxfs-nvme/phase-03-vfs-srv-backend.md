# Phase 03 — VFS /srv RedoxFS Backend

**Status**: Planned
**Priority**: High
**Blocked by**: Phase 01 (VirtIO-BLK), Phase 02 (RedoxFS fork)

---

## Context Links

- `cells/services/vfs/src/manager.rs:48-52` — `/srv` currently mounts `StubBackend`
- `cells/services/vfs/src/backend_stub.rs` — replace with `backend_redoxfs.rs`
- `cells/services/vfs/src/mount.rs` — `FsBackend` trait (the interface to implement)
- `libs/api/src/block.rs:10-26` — `ViBlockDevice` trait
- `libs/api/src/syscall.rs` — syscall enum (Law 1 — **2× confirmation required**)
- `libs/api/src/manifest.rs` — `MANIFEST_FLAG_*` constants (Law 1 — **2× confirmation required**)
- `third_party/redoxfs/` — forked crate (from Phase 02)

---

## Overview

Wire RedoxFS into the VFS `/srv` mount point. Three parts:
1. **`BlkWriteAsync` syscall (213)** — new kernel syscall for sector writes (mirrors `BlkReadAsync`).
   **Law 1**: requires 2× user confirmation before touching `libs/api/src/syscall.rs`.
2. **`VicellDisk`** — adapts `BlkReadAsync`/`BlkWriteAsync` to the RedoxFS `Disk` trait.
3. **`RedoxFsBackend`** — implements `FsBackend` using `FileSystem<VicellDisk>`.

---

## Law 1 Changes — CONFIRM BEFORE IMPLEMENTING

### Change A: `BlkWriteAsync` syscall 213

File: `libs/api/src/syscall.rs`

```rust
/// 213: Write one 512-byte sector from a Grant buffer into the active block device.
/// Returns 1 on success, 0 on error/permission denied.
BlkWriteAsync { sector: u64, grant_id: usize },
```

Also add to `kernel/src/task/syscall.rs` handler: mirror the `BlkReadAsync` gate
(BlockIoCap check + sector range check + grant ownership + min 512 bytes), then call
`get_block_device().write_sector(sector, buf)`.

**Must confirm with user before modifying `libs/api/src/syscall.rs`.**

### Change B: `MANIFEST_FLAG_PART_SRV` (bit 8)

File: `libs/api/src/manifest.rs`

```rust
/// Grants access to the /srv block partition (RedoxFS volume).
pub const MANIFEST_FLAG_PART_SRV: u64 = 1 << 8;
```

Add to the VFS service manifest (`cells/services/vfs/vicell.toml`):
```toml
[capabilities]
PART_SRV = true
```

**Must confirm with user before modifying `libs/api/src/manifest.rs`.**

---

## Architecture

```
VicellDisk::read_at(block, buf):
  1. sys_grant_alloc(1 page)  → grant_id
  2. sys_blk_read_async(block, grant_id)  → 1/0
  3. sys_grant_slice(grant_id)  → ptr
  4. copy ptr[..512] → buf
  5. sys_grant_free(grant_id)

VicellDisk::write_at(block, buf):
  1. sys_grant_alloc(1 page)  → grant_id
  2. sys_grant_slice(grant_id)  → ptr
  3. copy buf → ptr[..512]
  4. sys_blk_write_async(block, grant_id)  [NEW syscall 213]
  5. sys_grant_free(grant_id)
```

Performance note: Grant alloc/free per-sector is expensive for bulk operations.
Phase 04 can introduce a per-`VicellDisk` persistent Grant (GrantRegister/Unregister)
to amortize allocation. Not required for Phase 03 correctness.

---

## Related Code Files

| File | Action |
|------|--------|
| `libs/api/src/syscall.rs` | Add `BlkWriteAsync` variant (Law 1 — confirm first) |
| `libs/api/src/manifest.rs` | Add `MANIFEST_FLAG_PART_SRV` (Law 1 — confirm first) |
| `kernel/src/task/syscall.rs` | Add `BlkWriteAsync` handler (mirrors `BlkReadAsync`) |
| `cells/services/vfs/Cargo.toml` | Add `redoxfs = { path = "../../third_party/redoxfs", default-features = false }` |
| `cells/services/vfs/src/disk_virtio.rs` | Create — `VicellDisk: Disk` impl (syscall-backed) |
| `cells/services/vfs/src/backend_redoxfs.rs` | Create — `RedoxFsBackend: FsBackend` impl |
| `cells/services/vfs/src/manager.rs` | Replace `StubBackend` with `RedoxFsBackend` at `/srv` |
| `cells/services/vfs/src/lib.rs` | Add `mod disk_virtio; mod backend_redoxfs;` |
| `cells/services/vfs/vicell.toml` | Add `PART_SRV = true` to capabilities |

---

## Implementation Steps

1. **Obtain Law 1 confirmation** for both `libs/api/` changes. Do not edit `libs/api/`
   until confirmed.

2. **Add `BlkWriteAsync` to `libs/api/src/syscall.rs`** (post-confirmation):
   Same shape as `BlkReadAsync`, variant 213.

3. **Add kernel handler in `kernel/src/task/syscall.rs`**:
   ```rust
   Syscall::BlkWriteAsync { sector, grant_id } => {
       // identical gate sequence as BlkReadAsync
       // then: get_block_device().write_sector(sector, buf).map(|_| 1).unwrap_or(0)
   }
   ```

4. **Add `MANIFEST_FLAG_PART_SRV`** to `libs/api/src/manifest.rs` (post-confirmation).
   Update `kernel/src/task/syscall.rs` block-access gate to recognise bit 8.

5. **`VicellDisk`** (`cells/services/vfs/src/disk_virtio.rs`):
   ```rust
   pub struct VicellDisk { sector_count: u64 }
   impl Disk for VicellDisk {
       type Error = ViError;
       fn read_at(&mut self, block: u64, buf: &mut [u8]) -> Result<(), Self::Error> { ... }
       fn write_at(&mut self, block: u64, buf: &[u8]) -> Result<(), Self::Error> { ... }
       fn size(&self) -> Result<u64, Self::Error> { Ok(self.sector_count * 512) }
   }
   ```
   Use existing `sys_grant_alloc/slice/free` + new `sys_blk_write_async`.

6. **`RedoxFsBackend`** (`cells/services/vfs/src/backend_redoxfs.rs`):
   ```rust
   pub struct RedoxFsBackend {
       fs: FileSystem<VicellDisk>,
   }
   impl FsBackend for RedoxFsBackend {
       fn read_to_vec(&self, path: &str) -> Vec<u8> { /* fs.open(path)?.read_to_end() */ }
       fn write(&mut self, path: &str, data: &[u8]) -> bool { /* fs.open_create(path)?.write_all() */ }
       fn list(&self, path: &str, out: &mut [u8]) -> usize { /* fs.read_dir(path) */ }
       fn stat(&self, path: &str) -> Option<(u64, bool)> { /* fs.metadata(path) */ }
       fn file_size(&self, path: &str) -> u64 { /* fs.metadata().size */ }
       fn mkdir(&mut self, path: &str) -> bool { /* fs.create_dir(path) */ }
       fn unlink(&mut self, path: &str) -> bool { /* fs.remove_file(path) */ }
       fn rmdir(&mut self, path: &str) -> bool { /* fs.remove_dir(path) */ }
       fn rmdir_recursive(&mut self, path: &str) -> bool { /* fs.remove_dir_all(path) */ }
       fn append(&mut self, path: &str, data: &[u8]) -> bool { /* open + seek_end + write */ }
       fn get_file_ptr(&self, _path: &str) -> Option<(usize, usize)> { None } // no zero-copy for RedoxFS
   }
   ```
   Mount: `FileSystem::open(disk, false)` where `false` = not read-only. If open fails
   (unformatted), log a warning and return early — do NOT mkfs at runtime; disk must be
   pre-formatted (see Phase 04).

7. **`manager.rs`**: replace:
   ```rust
   // Before
   let srv = mounts.add_backend(Box::new(StubBackend::new()));
   mounts.mount("/srv", srv, false);

   // After
   let srv = mounts.add_backend(Box::new(
       RedoxFsBackend::mount("/srv").unwrap_or_else(|_| {
           log::warn!("[vfs] /srv: RedoxFS open failed, staying stub");
           // fall back to stub if no disk present (e.g. rv64 unit-test kernels)
           Box::new(StubBackend) as Box<dyn FsBackend>
       })
   ));
   mounts.mount("/srv", srv, true);  // writable
   ```
   This keeps non-disk CI runs functional.

---

## Todo

- [ ] Obtain Law 1 confirmation ×2 from user for syscall.rs + manifest.rs
- [ ] Add `BlkWriteAsync` (213) to `libs/api/src/syscall.rs`
- [ ] Add `MANIFEST_FLAG_PART_SRV` to `libs/api/src/manifest.rs`
- [ ] Implement kernel `BlkWriteAsync` handler in `kernel/src/task/syscall.rs`
- [ ] Add `redoxfs` path dep to `cells/services/vfs/Cargo.toml`
- [ ] Create `cells/services/vfs/src/disk_virtio.rs` (`VicellDisk`)
- [ ] Create `cells/services/vfs/src/backend_redoxfs.rs` (`RedoxFsBackend`)
- [ ] Update `cells/services/vfs/src/manager.rs` to use `RedoxFsBackend` with fallback
- [ ] Update `cells/services/vfs/vicell.toml` capabilities
- [ ] `cargo check -p service-vfs` passes

---

## Success Criteria

- `cargo check -p service-vfs --target riscv64gc-unknown-none-elf -Z build-std=core,alloc` exits 0
- QEMU boot with pre-formatted `srv.img` (see Phase 04): `/srv` mounts; `vcat /srv/hello.txt`
  returns expected content
- QEMU boot without `srv.img` (or with unformatted disk): `/srv` falls back silently to stub;
  boot completes normally (no regression for existing CI)
- All `libs/api/` changes have received 2× user confirmation
