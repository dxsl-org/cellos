## Slice: StorageVfs — files read: ~95 (VFS core 35 by me; cellos-fs/backends 13 and block drivers 50 via two read-only sub-audits, key lines re-verified) — unsafe blocks reviewed: ~61

### Findings

#### [S-1] FAT block stream has no partition bound — crafted FAT32 on P1/P6 reads/writes other partitions — Severity: High — STRIDE: T/I — OWASP: A08
- Where: cells/services/vfs/src/block_stream.rs:51-61, 66-93, 107-121; cells/services/vfs/src/backend_fat.rs:74; contrast cells/services/vfs/src/disk_cellosfs.rs:27-46; libs/api/src/abi/disk.rs:20-22, 46-56
- What: `BlockStream` issues `blk_read/blk_write(dev, self.base_lba + sector)` where `sector = pos / 512` and `pos` comes from fatfs (`SeekFrom::Start(n)` unchecked). No comparison against `PART_FAT32_SECTORS`/`PART_CELLSTORE_SECTORS` exists, unlike `CellosPartitionDisk` which rejects `block >= total_blocks`. fatfs derives cluster→sector from the untrusted BPB (`total_sectors`, FAT chain values) [INFERENCE on fatfs internals, from sub-audit].
- Exploit: A FAT32 image on P1 (`/mnt/sd`, the operator-editable interop volume) with an oversized `total_sectors_32` or a cluster chain pointing past LBA 526 336: any cell reading that file reads P2 cell table / P4 `/data` / P5 `/srv` (KMS, trust, machine-pin stores); any cell writing it (all cells may write `/mnt/sd`) overwrites P5/P6 cell-store ELFs (unsigned cells admitted by default per docs/security-model.md:45). Production.
- Fix: Give `BlockStream` a `sectors` field; refuse `sector >= sectors` and `base_lba.checked_add(sector)` overflow in `read`/`write`/`read_raw_sector`/`write_raw_sector`; pass partition sizes from `manager/disks.rs`.

#### [S-2] Any cell can poison a whole CellosFS mount (`/srv`, `/data`) with one bad write; inode numbers are never recycled so the volume dies permanently after 254 creates — Severity: High — STRIDE: D — OWASP: A04
- Where: cells/services/vfs/src/backend_cellosfs.rs:177-203 (`mutate` poisons on *every* `Err`, vs `read` 159-175 which poisons only on IoError/Corrupted/PartitionOverflow), 204-211, 330-341; libs/cellos-fs/src/engine.rs:287-334 (`create_entry`: `lookup(parent)?` → NotFound; `next_inode_num` monotonic, `> 256` → NoSpace), 627-660 (`unlink` never returns the inode number); libs/cellos-fs/src/inode.rs:80-96 (7 entries/dir: 428/56); format.rs:31, 42, 165
- What: `Write("/data/nodir/x")`, `Mkdir` of an 8th entry in any directory, a leaf name > 46 bytes, or `Append` to a directory each returns a namespace error from the engine, which `mutate` converts to `NativeState::Poisoned`; every later op on that mount returns `ERR_IO` until reboot. Separately, `next_inode_num` only ever increases and unlink does not free slots, so after 254 creates (files, dirs, and every D03 `stage_create` for conditional saves / `TrustCommit`) all creates fail `NoSpace` → poison, and this persists across reboot.
- Exploit: Any cell with `/data/` or `/srv/` write rights (all cells, rules.rs:80-84, 120-124) sends 8 `Write`s into one directory, or 254 `Write`+`Unlink` pairs. `/srv` hosts the KMS store, SSH trust store and machine pin; all become unavailable. Production.
- Fix: In `mutate`, poison only on `IoError | Corrupted | PartitionOverflow`, return `false` otherwise (pre-validate parent/name/capacity like `rmdir_recursive` does at 401-439). In the engine, maintain an inode free list (or scan `inode_blocks` for `INODE_MODE_UNUSED`) and reuse numbers; document the 7-entry/256-inode limits or add indirect directory blocks.

#### [S-3] NVMe register/doorbell MMIO is unbounded; stride comes from device-controlled `CAP.DSTRD` — Severity: High — STRIDE: T — OWASP: A04
- Where: cells/drivers/nvme/src/controller.rs:144-150, 368-389, 392
- What: Every access is `read/write_volatile(mmio.base() + off)` bypassing `MmioRegion` bounds; `db_stride = 4 << dstrd` with `dstrd` ∈ 0..15 from CAP. The I/O CQ doorbell sits at `0x1000 + 3*db_stride`; for `dstrd ≥ 11` it is ≥ 0x7000, past the 16 KiB window claimed (`main.rs:49`, sub-audit). AHCI by contrast routes through bounds-checked `MmioRegion` (ahci/src/controller.rs:947-955, sub-audit).
- Exploit: A controller advertising large DSTRD makes the driver write 32-bit values outside its granted BAR (cell death with device bus-mastering, or writes into adjacent MMIO if mapped).
- Fix: Use `MmioRegion::read_u32/write_u32`; at init reject `REG_DB_BASE + 3*db_stride + 4 > region.len()`.

#### [S-4] NVMe trusts Identify-Namespace LBA format: unchecked shift, no 512-byte check, single-page PRP — Severity: High — STRIDE: T — OWASP: A04
- Where: cells/drivers/nvme/src/controller.rs:311-319, 510-518; cells/drivers/nvme/src/main.rs:112-120 and dispatch.rs:65-77 (sub-audit)
- What: `lba_ds` is a raw device byte; `1u32 << lba_ds` overflows for ≥ 32. `lba_bytes` is never compared with the 512-byte wire sector; every READ/WRITE sends NLB=0 with only PRP1 (one 4 KiB `io_buf`). A namespace with 4 KiB blocks silently transfers 4096 bytes into a 512-byte protocol; ≥ 8 KiB blocks make the device follow `prp2 = 0` → DMA to IOVA 0 ([INFERENCE] faults under IOMMU, corrupts under identity map).
- Exploit: 4Kn or crafted namespace on real hardware/malicious device.
- Fix: Require `lba_ds == 9` (refuse the namespace otherwise); bounds-check before shifting.

#### [S-5] Unbounded `ReadAsync` pending table; each `/srv` pending read pins a namespace lease forever — Severity: Medium — STRIDE: D — OWASP: A04
- Where: cells/services/vfs/src/pending.rs:34-53 (no per-caller or global cap; contrast save.rs:149 `MAX_SAVES`); dispatch.rs:107-139; namespace.rs:79-111, 113-149 (shared lease blocks `reserve_one/two`); paths.rs:132-142 (`unlink_file` needs reservation); dispatch_paths.rs:73-80, 100-110, 169-171
- What: `ReadAsync` reads the whole file into a `Vec` and inserts it into `PendingTable` with no limit; the slot (and its `ServiceHandle` lease for `/srv` paths) lives until polled. A held lease makes every `Unlink`/`Rmdir`/`Mkdir`/`Rename` on that key fail `ERR_DENIED` for all cells.
- Exploit: One cell issues `ReadAsync` on every readable `/srv` file and never polls: VFS heap fills (16 MiB, `main.rs:122`) and the `/srv` namespace becomes undeletable/unrenamable; also 32 `OpenFileAt` handles per cell pin leases the same way.
- Fix: Cap pending slots per caller (e.g. 8) and globally; evict oldest or refuse; consider making leases advisory for unlink by another writer.

#### [S-6] Whole-file `read_to_vec` with no size cap on every inline/grant read path — Severity: Medium — STRIDE: D — OWASP: A04
- Where: cells/services/vfs/src/backend_cellosfs.rs:298-313 (`vec![0; inode.size]`); backend_fat.rs:322-346 (grows until EOF); callers dispatch.rs:133, dispatch_dirs.rs:189, grant_read.rs:76, grant_write.rs:71-80 (read-modify-write of the whole file per 4 KiB grant write), trust.rs:95
- What: A read of a ≥ ~12 MiB file (quota allows 32 MiB/cell, quota.rs:27; CellosFS size validated only against 64 MiB volume) allocates the full file in the 16 MiB VFS heap → OOM abort of the VFS cell. `WriteGrant` additionally copies the whole file per chunk.
- Exploit: Any cell appends a large file to `/data` then `ReadAt`s it; or a crafted image inode with a large `size`.
- Fix: Reject `read_to_vec` above a mount-level cap; serve `ReadAt`/`ReadFileGrant` via `read_at` ranges; make `WriteGrant` use `write_at`.

#### [S-7] CellosFS allocator sizes its bitmap from the on-disk `free_bitmap_blocks` — Severity: Medium — STRIDE: D — OWASP: A04
- Where: libs/cellos-fs/src/format.rs:159-200; libs/cellos-fs/src/allocator.rs:63-108 (sub-audit, not independently re-read)
- What: `validate_geometry` requires only `bitmap_end < total_blocks` and `bitmap_capacity >= total_blocks`; `load_from_disk` then allocates `bitmap_blocks * 4096`.
- Exploit: Crafted P4 superblock with `free_bitmap_blocks = 16_000` (CRC recomputed) → 64 MiB allocation at mount → VFS dies at boot.
- Fix: Require `free_bitmap_blocks == total_blocks.div_ceil(BLOCK_SIZE*8)`.

#### [S-8] No cross-inode block/inode ownership check on a crafted CellosFS image — Severity: Medium — STRIDE: T — OWASP: A08
- Where: libs/cellos-fs/src/engine.rs:535-574, 110-125, 627-660; allocator.rs:152-170 (sub-audit)
- What: Per-inode extent validation only; two inodes may map the same block, an extent may point at a bitmap-free block, two dir entries may share an inode. First free of a shared block makes it allocatable while still mapped elsewhere.
- Exploit: Crafted image: unlink file B sharing a block with the KMS/trust file A; the next write lands in A's content (silent substitution), or A's data leaks into a new file.
- Fix: On `open`, walk all reachable inodes (≤ 256), assert each extent block is set exactly once in the bitmap and each inode referenced once.

#### [S-9] FAT recursive delete recurses without bound; directory cluster cycles hang listing — Severity: Medium — STRIDE: D — OWASP: A04
- Where: cells/services/vfs/src/backend_fat.rs:200-233 (`remove_tree`), 574-580; contrast backend_cellosfs.rs:110-137 (depth 32 / 4096 entries)
- What: `remove_tree` recurses per subdirectory with no depth/entry limit; a crafted directory whose first cluster equals its parent's reproduces itself at every level → stack overflow. fatfs `DirIter` loops until an end marker so a chain cycle spins `list`/`stat` [INFERENCE].
- Exploit: Crafted SD/secondary disk + any cell's `RmdirRecursive`/`ListDir` on it.
- Fix: Apply the CellosFS `collect_tree` bounds; cap iteration count in `page_dir`/`stat`.

#### [S-10] virtio-blk x86 "poison" never sets `poisoned`; stalled request's bounce slots leak until `claim_slot` asserts — Severity: Medium — STRIDE: D — OWASP: A04
- Where: cells/drivers/virtio-blk/src/io_x86.rs:34-53 (`poison` only prints), 58-79; pci_x86.rs:152 (only write, `false`); hal_x86.rs:30-32 (`BOUNCE_SLOTS = 6`, assert per sub-audit)
- What: After a deadline expiry / `DEVICE_NEEDS_RESET`, `usable()` still returns Ok, so the next VFS request is issued on a queue whose stalled descriptors and 3–4 bounce slots were never recycled; `pop_used` returns `WrongToken` on the late completion, every later request fails `Indeterminate` and leaks more slots; within ~2 requests `claim_slot` asserts (`panic = abort`) and the driver cell dies with the device bus-mastering.
- Exploit: Device withholds one completion > 30 s, then ordinary VFS traffic.
- Fix: `self.poisoned = true;` inside `poison()`.

#### [S-11] NVMe timeout abandons an in-flight command while the single `io_buf` is reused by the next request — Severity: Medium — STRIDE: T — OWASP: A04
- Where: cells/drivers/nvme/src/controller.rs:528-541 (timeout → `Indeterminate`, no state change); dispatch.rs:65-88, queue.rs:84-106 (sub-audit)
- What: Controller keeps running; the next request points PRP1 at the same page. A late READ completion DMAs stale data over the next WRITE's payload or READ result (integrity, not memory safety — buffer stays authorized). AHCI stops/restarts or faults (ahci/src/controller.rs:682-710); virtio-scsi poisons (queue.rs:118-121).
- Fix: Set a `faulted` flag and refuse I/O, or reset (`CC.EN=0`, wait `RDY=0`) before reuse.

#### [S-12] Zero-byte files and directories bypass quota; ramfs `/tmp` and the `writers` map grow without bound — Severity: Medium — STRIDE: D — OWASP: A04
- Where: cells/services/vfs/src/paths.rs:90-99 (`net_charge == 0` skips `can_charge`); dispatch_paths.rs:69-90 (`Mkdir` has no quota); quota.rs:99-101 (`writers: BTreeMap<String, CellId>` grows per path); backend_ramfs.rs:148-152, 278-288 (no entry limit)
- What: Each empty `Write`/`Mkdir` adds a ramfs node plus a `writers` entry at zero cost; nothing bounds node count.
- Exploit: Any cell loops `Write("/tmp/<n>", "")`/`Mkdir` → VFS heap exhaustion (OOM abort of the shared service).
- Fix: Charge a per-entry minimum (e.g. 256 B like `OUTCOME_CHARGE`) for files and directories; cap entries per cell.

#### [S-13] Path-keyed ACL gives every cell read/write/delete over every other cell's files, including in-flight D03 staging objects — Severity: Medium — STRIDE: I/T — OWASP: A01
- Where: cells/services/vfs/src/access/rules.rs:8-19 (documented: rules "never keyed by CellId"), 80-101, 120-124; backend_cellosfs.rs:85, 487-489 (`/{prefix}/.cbx-save/<id>` under the all-writable `/srv/`/`/data/` rules); access.rs:139-168
- What: `/data/`, `/tmp/`, `/mnt/sd/`, `/mnt/disk/`, `/srv/` are `allow_read_all && allow_write_all`; handles only narrow *naming*, not rights (dispatch_dirs.rs:9-13). Any cell can overwrite/unlink another cell's files and its `.cbx-save/<id>` stage (the seal's `(inode, generation)` check at backend_cellosfs.rs:554-569 and engine.rs:233-238 turns tampering into `STAGE_CHANGED`, so integrity holds but availability does not). Quota refund goes to the recorded writer, so the attacker is only charged, never credited (paths.rs:112-118).
- Exploit: `Unlink("/data/.cbx-save/00000001")` or `Write` over another cell's config between its `SaveSeal` and `SaveCommit`; overwrite another cell's `/data` state.
- Fix: Acknowledged design gap; at minimum make `.cbx-save` paths owner-only (deny path access, reachable only through the save API), and per-cell subtrees under `/data/<cell-label>` once kernel-vouched labels exist.

#### [S-14] One sector cache per FAT mount, each budgeted at ¼ of the heap — Severity: Low — STRIDE: D — OWASP: A04
- Where: cells/services/vfs/src/page_cache.rs:16-31 (`MAX_CACHE_BYTES = VFS_HEAP_BYTES/4`, per instance), 121-128; block_stream.rs:175-178; manager/disks.rs:72-82, 96-102; backend_bin_overlay.rs:33
- What: `/mnt/sd` and the `/bin` cell-store each own a cache; every secondary disk adds another. With ≥ 2 secondaries the combined eviction watermark (4 × 3.6 MiB) exceeds the 16 MiB heap [INFERENCE on exact OOM point].
- Fix: One shared `PageCache` or a global budget divided among mounts.

#### [S-15] `GetFile` `DataPtr` dangles after the ramfs file is rewritten — Severity: Low — STRIDE: I — OWASP: A08
- Where: cells/services/vfs/src/backend_ramfs.rs:175-181, 144-147 (`existing.data = Vec::from(content)` frees the old buffer); dispatch_paths.rs:15-27; fast_handler.rs:11-14
- What: A cell holding a `DataPtr` into `/tmp/x` reads freed VFS heap after any cell overwrites `/tmp/x` (all cells may). In the SAS tier that is a read of whatever the VFS heap reallocates (other cells' pending reads, trust-store bytes).
- Fix: Only hand out pointers for immutable backends, or pin/refcount the buffer until a `Close`.

#### [S-16] Guest VM disk image is world-readable — Severity: Low — STRIDE: I — OWASP: A01
- Where: cells/services/vfs/src/access/rules.rs:58-62; access.rs:149-153
- What: `/mnt/sd/guest_disk.img` (and its 8.3 aliases, guest_disk.rs:43-54) allows `Read` for any cell; only writes are confined to the live hypervisor.
- Fix: Restrict reads to the hypervisor as well.

#### [S-17] AHCI data command success does not check PRDBC — Severity: Low — STRIDE: T — OWASP: A04
- Where: cells/drivers/ahci/src/controller.rs:589-603 vs 360-371 (sub-audit)
- What: A READ that clears `PxCI` with a short/zero transfer is reported `Completed`, returning stale `io_buf` contents.
- Fix: Require command-header PRDBC `== 512`.

#### [S-18] Minor correctness — Severity: Low — STRIDE: - — OWASP: -
- cells/drivers/disk/src/lib.rs:45-47, 60-62: `sector * SECTOR_SIZE` / `offset + len` unchecked (no in-tree consumer found; sub-audit).
- libs/cellos-fs/src/engine.rs:85-91: a CRC-valid but geometry-invalid superblock slot refuses the mount instead of falling back to the other slot (sub-audit).
- cells/services/vfs/src/backend_fat.rs:422-423: `dir.remove(name)` before `create_file` turns an empty directory into a file via `Write`.
- cells/services/vfs/src/manager/state_transfer.rs:40-63: `deserialize_state` never checks `VFS_SCHEMA_VERSION`.
- cells/services/vfs/src/fast_handler.rs:34 + main.rs:215-226: the fast path takes `GLOBAL_VFS.lock()` with interrupts disabled while the main loop holds it across blocking disk IPC — spin/deadlock if ever wired (kernel/src/fast_ipc.rs:156-161 says the path is unreachable today).
- cells/drivers/nvme/src/dispatch.rs:58-63: out-of-range LBA logged but still issued (sub-audit).
- libs/cellos-fs/src/format.rs:359 comment says 258-byte entries; actual 56. CRC32C (crc32.rs) is integrity-only, not an authentication boundary.

### Verified-safe areas
- Path canonicalisation: `is_canonical_policy_path` rejects relative, `.`/`..`, `//`, embedded empty components (access/kms.rs:25-46); every `decide` runs it (access.rs:146-148); handle names validated as raw bytes (`validate_dir_component`, libs/api/src/services/dir_name.rs:67-89: no `/`, `\`, control chars, `.`/`..`, ≤ 255) and roots via `validate_dir_path` (103-119); `join_component` cannot escape (126-132). Backends additionally reject `..` (backend_cellosfs.rs:141-147; backend_fat.rs:289, 489, 538, 577).
- Mount prefix matching is boundary-aware (mount.rs:54-72); `/srvx` does not reach `/srv`.
- Central mutate gate before any path/handle work (dispatch.rs:57-62), machine-pin exception narrowed to canonical `/srv/.cellos-machine` paths from the live provider (access.rs:90-94, kms.rs:48-56); sealed cells refused for all path-addressed ops at entry (dispatch.rs:67-69).
- Handle IDOR: dir/file/pending/cap tables all filter by `owner == caller` (cell+generation) and answer unknown/not-yours identically (dirs.rs:179-183; file_handles/table.rs:104-117, 142-176; pending.rs:59-81; handle_table.rs:67-93); IDs are monotonic, never reused (dirs.rs:191-199; table.rs:81-85); per-caller caps 32 (dirs.rs:37; table.rs:11); transitive revocation (lifecycle/revoke.rs:10-40) and file-handle tombstoning on parent close (dispatch_dirs.rs:80-86); generation replacement purges predecessor state (lifecycle.rs:68-76; dir_admission.rs:23-40).
- Inherited handle binding is all-or-nothing against the spawner's *held* entries and seals the child either way (dirs/bind.rs:54-98; dir_admission.rs:57-74); kernel record is provenance only (kernel/src/task/dir_inherit.rs:46-71, 75-90).
- Grant I/O: kernel binds `grant_owner == vfs_context.grant_owner && shared_to == VFS` when a VFS request context exists (kernel/src/task/syscall.rs:3594-3601); VFS re-authorises the handle path and bounds `n` by `data_len`, grant length and 4096 (grant_read.rs:27-47); `WriteGrant` bounds `bytes ≤ 4096`, `offset ≤ file_len`, checked_add, quota before copy (grant_write.rs:42-66, 84-86); `WriteHandleGrant` refuses guest-disk growth and overflow (dispatch_grants.rs:69-83).
- Quota: delete credits the recorded writer, never the deleter (paths.rs:112-155; quota.rs:117-130); overwrite charges net delta only for the same writer (paths.rs:86-99).
- Boot configuration override files protected case-insensitively incl. ancestors (access.rs:143-145, 200-215; manager.rs:350-365); provider namespaces (`/srv/cellos`, `/srv/.cellos-trust`, `/srv/.cellos-machine`) deny-all except live provider (rules.rs:38-63, 105-119; access.rs:155-161); `Rename` confined to `/srv` with both endpoints authorised and reserved (dispatch_paths.rs:144-183).
- D03 save: seal digests storage, not request bytes (save.rs:234-261); publication compares `(inode, generation)` under the engine lock and every mutation bumps generation (backend_cellosfs.rs:548-569; engine.rs:233-238); bounded per caller/global (save.rs:143-151).
- Prepared-cwd logs/reservations bounded and quota-charged (prepared_cwd.rs:24-30, 191-196, 359-364, 950-962).
- SSH trust store never leaves the service; NET_CONNECT flag required; size/entry limits (trust.rs:30-37, 78-104).
- CellosFS: superblock magic+CRC+geometry with checked arithmetic; inode slot/number consistency; dir-entry validation; `validate_write_range`; `BoundedDisk` and `CellosPartitionDisk` bounds (format.rs:116-200; engine.rs:143-172, 174-231, 337-367, 370-404; disk.rs:34-122; disk_cellosfs.rs:27-46); recursive removal bounded (backend_cellosfs.rs:110-137, 401-439).
- Page cache is device-first write-through, failed write evicts (page_cache.rs:93-105).
- Drivers: virtio-scsi used-ring `id`/completion count checked, `len` ignored, fences correct (virtio-scsi/src/queue.rs:84-125); sense/residual/capacity parsing bounded (proto.rs:113-134; libs/scsi-block/src/exec.rs:116-131, parse.rs:93-135, disk.rs:130-158, sense.rs:62-89); virtio-pci-transport cap/BAR/notify bounds in u64 (caps.rs:92-187; transport/mod.rs:60-85, 174-179; ops.rs:50-72, 160-178); AHCI MMIO bounds-checked, ports ≤ 32, LBA refused ≥ capacity, fatal bits checked, timeout stops engine (ahci/src/controller.rs:225, 434-449, 589-603, 630-633, 682-710, 947-955); NVMe CQ phase/CID handling (nvme/src/queue.rs:84-106, 196-219); all four block cells accept requests only from the registered VFS tid (virtio-blk/main.rs:58-68, virtio-scsi/main.rs:55-65, ah…

### Not covered
- cells/services/vfs/src/prepared_cwd.rs beyond lines 1-140, 745-987 (state machine of reconcile/complete_pending/reclaim: 988-1707 not read); prepared_cwd/closed_logs.rs; manager/{listing,change_marks,owned_state}.rs; dirs.rs tests; trust/first.rs beyond line 143 (commit path); trust.rs parse beyond line 123; backend_bootfs.rs, backend_bin_overlay.rs (only `get_file_ptr`/mount call sites); blk_router/{router,requests}.rs beyond name/lookup; access/machine_pin.rs.
- libs/cellos-fs/src/cow_experiment.rs (feature `g2-cow-experiment`, host-only) and tests/ (skimmed for claimed invariants only); libs/virtio-pci-transport/src/tests/**, libs/scsi-block/tests/**; kernel `MmioRegion`/`request_region` implementation; kernel/src/task/vfs_lifecycle_selftest.rs beyond lines 1-83 (selftest-only).
- tools/cellos-fs-provision: only main.rs:52-103, 218-225 via sub-audit (host dev tool; small TOCTOU and unbounded recursion in `seed_tree`, not production).

[Some lines truncated to 768 chars. Use agent://StorageVfs-2:raw:125-125 to read line 125 whole]