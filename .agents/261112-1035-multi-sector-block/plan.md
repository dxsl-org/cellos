# Plan — multi-sector block reads

Goal: a block read moves up to 64 KiB per IPC round trip instead of one 512-byte sector, so
files on `/mnt/sd` (first user: the Tier 3 Chrome guest image) read at tens of MB/s, not ~2 MB/s.

Trigger: Tier 3 Chrome phase 4 (`53b0d3200`): Chromium's UI appears after ~18 min; guest `dd`
48 MiB takes 23-25 s; the guest reports soft lockups. Orientation: [scout-report.md](scout-report.md).

## Design (chosen; superseded in phase 01 — inline `OP_READ_RUN`, see its deviation log)

New wire op `OP_READ_GRANT` beside the existing ones:

- request (24 B, LE): `op:u16=3, count:u16, reserved:u32=0, sector:u64, grant:u64`;
  reply: 1 byte `BlockOutcome`, all-or-nothing (no partial counts).
- `grant` is a VFS-owned Grant (64 KiB, one per device, allocated at discovery), shared
  ReadWrite with that device's driver. The driver resolves it owner-checked
  (`sys_grant_slice_with_len_owned_by(grant, vfs_tid)`), refuses `count` outside
  `1..=MAX_READ_SECTORS` (128) or past the Grant, reads in device-sized pieces into its own DMA
  area and copies into the Grant. Drivers cannot DMA into another Cell's memory (only
  `XE_GPU` holds DMA exports), so the copy stays.
- VFS: `CachedBlockStream::read` serves a sector-aligned request of ≥ 2 sectors with one
  `OP_READ_GRANT` (fatfs already asks for up to a cluster per read) and bypasses the sector
  cache for that bulk data (the cache is write-through device-first, so it never holds bytes
  that differ from the device).
- Writes stay single-sector (out of scope: no measured need).

Rejected: inline multi-sector replies (4096 B IPC ceiling → 7 sectors, 8x not 128x);
DMA straight into the VFS Grant (needs a new kernel DMA-export holder class); a hypervisor-side
whole-image preload (still 2 MB/s, hides the problem for one user).

## Phases

| # | Phase | Outcome | Status |
|---|---|---|---|
| 01 | [Wire op, VFS client, NVMe](phase-01-wire-vfs-nvme.md) | `/mnt/sd` on NVMe reads via `OP_READ_RUN` (inline, 7 sectors; Grant design failed, see log) | completed |
| 02 | [NVMe large transfers](phase-02-nvme-large-transfers.md) | one NVMe command per ≤64 KiB (PRP list, MDTS) | dropped (runs fit one page) |
| 03 | [Remaining drivers, fallback removed](phase-03-other-drivers.md) | virtio-blk, AHCI, pvscsi, virtio-scsi serve the op; VFS fallback deleted | completed |
| 04 | [Hypervisor disk and Chrome startup](phase-04-hypervisor-chrome.md) | Chrome guest startup measured and bounded; deferred Tier 3 checks done | completed (UI after 29–32 s, KVM) |

Dependencies: 02 and 03 need 01; 04 needs 01 (02 improves it).

## Top risks

1. Another process edits `cells/services/vfs/src/{main,blk_router}.rs` concurrently: re-read
   before every edit, keep changes in `blk_router/requests.rs`, `block_stream.rs` and new files.
2. A driver that copies past the Grant or trusts `count` would corrupt VFS memory: bounds are
   checked against the owner-checked Grant length, with host tests on the request decoder.
3. NVMe PRP-list handling (phase 02) touches DMA authorization; a wrong PRP entry is a DMA
   outside the authorized range. The `nvme-dma-addressing` lane gates it.

## Evidence

Logs and screenshots go to `.agents/261112-1035-multi-sector-block/evidence/` (not shipped).
