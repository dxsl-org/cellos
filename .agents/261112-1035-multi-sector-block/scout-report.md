# Scout report — multi-sector block reads

Date: 2026-11-12. Sources: direct reads + one Explore scout (BlockDriverMap). Unverified = [INFERENCE].

## Measured problem

- Tier 3 Chrome guest disk (`/mnt/sd/chrome-guest.sqfs`, FAT32 on NVMe) reads at ~2 MB/s:
  guest `dd` 48 MiB in 23-25 s, with 4 KiB and 64 KiB FAT clusters alike. Chromium UI appears
  after ~18 min; guest reports soft lockups (vCPU blocked in synchronous I/O).
  Evidence: `.agents/261111-1700-tier3-chrome/phase-04-chromium-guest.md` § Deviation Log.

## Read path today (one 512-byte sector per IPC round trip)

| Layer | Where | Unit |
|---|---|---|
| Hypervisor virtio-blk | `cells/services/hypervisor/src/virtio_blk.rs` `blk_read` | one VFS `ReadHandleGrant` per ≤64 KiB chunk (`api::ipc::VFS_GRANT_CHUNK`) |
| VFS grant read | `cells/services/vfs/src/dispatch_grants.rs:13` | `stat` + `read_at` (re-opens the file, seeks from cluster 0) |
| fatfs | rust-fatfs rev c4b8847 `src/file.rs:256` | asks the disk for up to one cluster per `read` |
| VFS stream | `cells/services/vfs/src/block_stream.rs` `CachedBlockStream::read` | returns **one sector** per call (fatfs loops) |
| Page cache | `cells/services/vfs/src/page_cache.rs` | LRU of 512-byte sectors, budget = heap/4 (4 MiB of 16 MiB) |
| Router | `cells/services/vfs/src/blk_router{.rs,/requests.rs,/router.rs}` | `post` request, masked `sys_recv` reply; `blk_read_sectors` loops single sectors |
| Wire | `libs/types/src/block_wire.rs` | read 10 B → reply 513 B; `OP_READ/WRITE/FLUSH` only |
| Drivers | nvme, virtio-blk, ahci, pvscsi, virtio-scsi | one sector per device command |

IPC ceiling: 4096 B per message (`libs/api/src/services/ipc.rs:23`, `kernel/src/ipc/ipc_wire.rs:17`).

## Drivers (BlockDriverMap)

| Driver | Read command | DMA area today | Room without new allocation |
|---|---|---|---|
| nvme | `submit_io(READ, nlb=0, prp1)` `controller.rs:594`; PRP1 only (`layout.rs:13-15`, `fits_one_prp`) | `BounceBuffer` 1 page (`bounce.rs:39-50`) | 8 sectors; more needs PRP2/PRP list; MDTS never read |
| virtio-blk | `read_blocks_nb` with `[u8;512]` (`io_x86.rs:83-96`, `dispatch.rs:49`) | HAL bounce pool 6 × 4 KiB (`hal_x86.rs:29-35`) | 8 sectors; seg_max/size_max not read |
| ahci | READ DMA EXT, count 1, PRDTL 1 (`controller.rs:489-516`) | `io_buf` 1 page (`main.rs:158-178`) | 8 sectors; command table has room for ~248 PRDs [INFERENCE] |
| pvscsi | scsi-block `READ(16)` (`libs/scsi-block/src/disk.rs:80-95`) | data 1 page (`rings.rs:86-88`) | 8 sectors; scsi-block already multi-block |
| virtio-scsi | same scsi-block path | data 1 page (`ring.rs:17-25`) | 8 sectors; reads seg_max/max_sectors |

- Request loop: every driver runs `ostd::run_app`, checks `sender_is_vfs`, answers with
  `ostd::ipc::reply_to` (nvme `main.rs:184`, virtio-blk `main.rs:120`, ahci `main.rs:244`,
  virtio-scsi `main.rs:175`, pvscsi `dispatch.rs:51`).
- No driver uses Grants. A Cell can DMA only into its own Grants (`sys_grant_dma`,
  `kernel/src/task/syscall/grant.rs:1810`); cross-Cell DMA export is limited to `XE_GPU`.
  So a driver must copy from its DMA area into a client Grant.
- Desktop image ships all five (`scripts/assemble-x86-image.py:123-148`).
- Lanes: `ahci-x86`, `pvscsi-x86`, `virtio-x86` (pr tier); `nvme-x86`, `nvme-dma-addressing`
  (manual). Host tests: nvme layout/queue, pvscsi proto/rings, types block_wire outcomes;
  no host test covers driver dispatch/read.

## Concurrency note

Another process is editing `cells/services/vfs/src/main.rs` and `blk_router.rs` (uncommitted
at 2026-11-12). Phases touching VFS must re-read those files and keep edits out of their hunks.
