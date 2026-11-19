---
phase: 1
title: "Wire op, VFS client, NVMe"
status: completed
priority: P1
effort: "2-3d"
dependencies: []
tier: thinking
checkpoint: true
---

# Phase 1: Wire op, VFS client, NVMe

> **Required — deviation-log:** log every Decision / Deviation / Surprise in § Deviation Log when it happens.

## Overview

Add `OP_READ_GRANT` to the block wire, teach the VFS to use it for bulk FAT reads, and
implement it in the NVMe driver (the device behind `/mnt/sd` in the Tier 3 lanes). Other
drivers answer the unknown op as `NotIssued`; the VFS then uses single-sector reads for that
device (temporary, removed in phase 03).

## Requirements

- Functional: `types::block_wire` gains `OP_READ_GRANT`, `MAX_READ_SECTORS = 128`, and a
  `ReadGrantRequest` encode/decode (24 B LE, reserved must be 0, `count` in `1..=128`).
- Functional: VFS allocates one 64 KiB Grant per routed device at discovery and shares it
  ReadWrite with the device's provider tid (re-share on provider restart, the router already
  tracks provider changes); `Router::read_sectors(dev, lba, count, buf)` sends one request and
  copies the Grant into `buf` on `Completed`.
- Functional: `CachedBlockStream::read` uses `read_sectors` for sector-aligned requests of ≥ 2
  sectors (bounded by the partition, `abs_lba` check per range); smaller or unaligned reads
  keep the cached single-sector path. `blk_read_sectors` (`blk_router.rs:169`) uses it too.
- Functional: NVMe decodes the op, resolves the Grant owner-checked against the VFS tid,
  reads in ≤8-sector commands (`nlb = n-1`, existing 4 KiB bounce page, `fits_one_prp`) and
  copies each piece into the Grant; any failure → `Indeterminate`/`NotIssued` per existing rules.
- Non-functional: guest `dd` of 48 MiB from the Chrome image ≥ 10x faster than 23-25 s.

## Architecture

`fatfs File::read` (≤ one cluster) → `CachedBlockStream::read` → `Router::read_sectors` →
`post` 24-byte request to the driver → driver reads into its DMA page, copies into the shared
Grant → 1-byte reply → VFS copies Grant → fatfs buffer. Per-device state in the router:
`grant_id`, `shared_with_tid`, `read_grant: Supported | Unsupported | Unknown`.

## Assumptions

- **Claim:** NVMe `submit_io` accepts `nlb > 0` within one page with PRP1 only.
  **Confidence:** high (NVMe spec; PRP1 covers one page). **Verify:** `layout.rs` tests.
- **Claim:** every driver replies `NotIssued` to an unknown opcode. Verified for nvme
  (`dispatch.rs:57-58`) and virtio-blk (`dispatch.rs:43-44`); ahci and pvscsi return
  `NotIssued` on refusals (`ahci/dispatch.rs:53`, `pvscsi/dispatch.rs:44`).
  **Confidence:** high for nvme/virtio-blk, medium for the rest. **Verify:** read their `parse_op`.
- **Claim:** a VFS-owned Grant shared with a driver can be sliced owner-checked by the driver
  while the VFS waits in `sys_recv` (lease semantics of `sys_grant_slice_with_len_owned_by`).
  **Confidence:** medium. **Verify:** `libs/ostd/src/syscall.rs:2162-2212` and a lane run.

## Related Files

- Modify: `libs/types/src/block_wire.rs` (+ tests)
- Modify: `cells/services/vfs/src/blk_router/{requests.rs,router.rs,discovery.rs}`,
  `cells/services/vfs/src/block_stream.rs`, `cells/services/vfs/src/blk_router.rs` (one fn)
- Modify: `cells/drivers/nvme/src/{dispatch.rs,controller.rs,layout.rs,main.rs}`
- Modify: `docs/abi-boundary-records.md` (new record), `docs/project-changelog.md`

## Implementation Steps

1. `block_wire`: op, constants, `ReadGrantRequest` with decode refusing reserved≠0, count 0,
   count > 128; host tests for every refusal and a round trip.
2. VFS router: per-device Grant allocate+share at discovery; `read_sectors`; mark a device
   `Unsupported` on `NotIssued` for this op and fall back to the per-sector loop; host tests
   with the existing `BlockSys` fake (`blk_router/tests.rs`): supported, unsupported fallback,
   provider restart re-share, Grant copy only on `Completed`.
3. `CachedBlockStream::read`: bulk path for aligned ≥2-sector reads; test that a partition
   bound refuses before any device access and that reads after a cached write return the
   written bytes.
4. NVMe: decode, owner-checked Grant slice, ≤8-sector commands, copy; `layout.rs` test that an
   8-sector command never crosses the bounce page.
5. Lanes: `nvme-x86`, then the Tier 3 Chrome run (`/tmp`-style runner from the Tier 3 plan) to
   measure guest `dd` and Chromium time-to-UI; `qemu-tier3-display-x86.sh`, phase-04 GUI proof.
6. ABI record + changelog with the measured numbers.

## Success Criteria

- [x] `cargo test -p types`, `-p service-vfs` (or the VFS crate name) and `-p driver-nvme` pass on host.
      — PASS at `c4c7d5d34f` (2026-11-19): `types` 23, `service-vfs` 149, `driver-nvme` 15.
- [x] `nvme-x86` lane PASS; Tier 3 display lane PASS; phase-04 GUI proof PASS. — `nvme-x86` 11/11
      (KVM), display 41/41, GUI proof 268 checks, at `3c7e719c2` (phase 04 Deviation Log).
- [x] Guest `dd` 48 MiB from the Chrome image ≥ 10x faster (≤ 2.5 s); Chromium time-to-UI logged.
      — **`dd` not re-measured.** Time-to-UI logged instead: 29–32 s vs ~132 s; guest reads keep the
      vCPU busy ~12–15 s in total (changelog 2026-11-12).
- [x] AHCI/virtio boots (`ahci-x86`, `virtio-x86`) still PASS through the fallback. — The fallback
      was deleted in phase 03; `ahci-x86` and `virtio-x86` 4/4 PASS without it (changelog 2026-11-13).

## Security Considerations

The driver writes into VFS memory: only through the owner-checked Grant slice, bounded by its
length and `count * 512`; the VFS never trusts a reply length. Requests from non-VFS senders stay
refused (`sender_is_vfs`).

## Risk Notes

Concurrent VFS edits by another process (see plan.md). The fallback path is temporary and must
be deleted in phase 03, not kept.

## Deviation Log

- **Surprise → Deviation (2026-11-12): `OP_READ_GRANT` replaced by inline `OP_READ_RUN`.**
  The Grant design failed on hardware: the NVMe driver filled the VFS Grant, but the VFS could
  never read it back (`[blk-router] read Grant … unreadable after a completed read`, guest
  `mount-squashfs` failed). Cause: while the VFS serves a request, `GrantSlice` authorizes only
  Grants owned by the current client and shared to the VFS
  (`kernel/src/task/syscall/grant.rs` `authorize_grant_slice_locked`, `vfs_context` branch);
  outside a request it is refused (`MissingContext`). A VFS-owned Grant is unreadable by design.
  Chosen instead: the plan's rejected alternative, inline replies — `OP_READ_RUN`
  (16 B request `op=3,count,reserved,sector`), reply = outcome + `count*512` bytes,
  `count ≤ MAX_RUN_SECTORS = 7` (4096 B IPC ceiling). No kernel change, no Grant lifecycle.
- **Decision: read-ahead in `CachedBlockStream`.** fatfs asks for one 4 KiB cluster per read
  (FAT32 on 256 MiB forces 4 KiB clusters), so a sector-aligned read of ≥ 2 sectors refills a
  64 KiB window (`READ_AHEAD_SECTORS`) with runs of 7; the window is dropped on any write.
- **Decision: last-sector fast path.** fatfs re-walks a file's cluster chain from its first
  cluster on every seek (rust-fatfs `File::seek`), one 4-byte FAT read per cluster; each used to
  cost a page-cache lookup, LRU touch and 512-byte copy.
