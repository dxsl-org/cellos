---
phase: 3
title: "Remaining drivers, fallback removed"
status: completed
priority: P2
effort: "2-3d"
dependencies: [1]
tier: medium
checkpoint: true
---

# Phase 3: Remaining drivers, fallback removed

> **Required — deviation-log:** log every Decision / Deviation / Surprise in § Deviation Log when it happens.

## Overview

virtio-blk, AHCI, pvscsi and virtio-scsi serve `OP_READ_GRANT`; the VFS per-device
`Unsupported` fallback from phase 01 is deleted, so every routed device has one read contract.

## Requirements

- Functional: each driver decodes the op with the shared `block_wire` decoder, resolves the
  Grant owner-checked, reads in pieces that fit its existing 4 KiB DMA area (8 sectors) or its
  reported limit (virtio-scsi `max_transfer_bytes`), and copies into the Grant.
- Functional: AHCI sector count > 1 (`controller.rs:510`), byte count `n*512-1` (`:516`).
- Functional: virtio-blk passes `n*512` buffers to `read_blocks_nb` (≤ one 4 KiB slot).
- Functional: VFS drops `read_grant: Unsupported` and the per-sector bulk fallback; a
  `NotIssued` from `OP_READ_GRANT` is a read failure like any other.

## Architecture

Shared helper in `libs/types` or a small `block-grant` module in `ostd` [decide in step 1;
log it]: `resolve(grant, vfs_tid, count) -> Option<&mut [u8]>` so the four drivers do not
each re-derive the bounds.

## Assumptions

- **Claim:** scsi-block `read` already accepts multi-block buffers up to `max_transfer_blocks()`.
  **Confidence:** high (`libs/scsi-block/src/disk.rs:44,80`). **Verify:** read it.
- **Claim:** virtio-drivers `read_blocks_nb` accepts multiples of 512. **Confidence:** medium.
  **Verify:** the crate source in `~/.cargo`.

## Related Files

- Modify: `cells/drivers/{virtio-blk,ahci,pvscsi,virtio-scsi}/src/{dispatch.rs,...}`
- Modify: `cells/services/vfs/src/blk_router/{requests.rs,router.rs}` (fallback removal)
- Modify: `docs/abi-boundary-records.md`, `docs/project-changelog.md`

## Implementation Steps

1. Shared Grant-resolve helper + host tests (count 0, count > max, Grant shorter than
   `count*512`, wrong owner).
2. One driver at a time: virtio-blk, AHCI, pvscsi, virtio-scsi; build each for x86.
3. Delete the VFS fallback and its tests; keep the refusal tests.
4. Lanes: `virtio-x86`, `ahci-x86`, `pvscsi-x86`, `nvme-x86`; Tier 3 display lane; phase-04 GUI proof.

## Success Criteria

- [x] Host tests for the helper and VFS router pass; x86 builds of all five drivers clean. —
      `types` 23, `service-vfs` 149, `scsi-block` 50, `driver-nvme` 15 PASS at `c4c7d5d34f`
      (2026-11-19); all five drivers built into the lane ISOs below.
- [x] `virtio-x86`, `ahci-x86`, `pvscsi-x86`, `nvme-x86` lanes PASS. — Under KVM: `virtio-x86` 4/4,
      `pvscsi-x86` 3/3, `ahci-x86` every non-skipped test (changelog 2026-11-13); `nvme-x86` 11/11
      (phase 04).
- [x] `grep` finds no single-sector bulk fallback left in `blk_router`. — `ReadRunSupport` absent
      from `cells/services/vfs/src` at `c4c7d5d34f`.

## Security Considerations

Same Grant bounds as phase 01, now in four more Cells: the shared helper is the only place that
turns a Grant id into a writable slice.

## Risk Notes

virtio-scsi has no dedicated lane (scout); if none can be added cheaply, log it and rely on its
host tests plus a manual QEMU boot with `-device virtio-scsi-pci`.

## Deviation Log

- **Deviation (2026-11-12): the op is `OP_READ_RUN`, served per sector inside the driver.**
  AHCI, virtio-blk, pvscsi and virtio-scsi answer a run by looping their existing one-sector
  read (`block_wire::serve_read_run`: all-or-nothing, range checked against the device size
  where the driver knows it). The saving is the VFS round trips (7 sectors per IPC); native
  multi-block commands in these drivers are a possible later speedup (review, LOW).
- **Decision: the VFS per-sector fallback is deleted** (`ReadRunSupport` gone); a provider that
  answers a run with anything but Completed fails the read. Drivers and VFS ship together.
