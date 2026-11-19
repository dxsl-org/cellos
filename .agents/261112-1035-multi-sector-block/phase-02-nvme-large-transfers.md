---
phase: 2
title: "NVMe large transfers"
status: dropped
priority: P2
effort: "1-2d"
dependencies: [1]
tier: thinking
checkpoint: false
---

# Phase 2: NVMe large transfers

> **Required — deviation-log:** log every Decision / Deviation / Surprise in § Deviation Log when it happens.

## Overview

Serve one `OP_READ_GRANT` (≤64 KiB) with one NVMe command instead of up to sixteen 4 KiB
commands: a 64 KiB DMA area, PRP2 / PRP list, and the controller's MDTS honoured.

## Requirements

- Functional: read `MDTS` from Identify Controller; max transfer = min(64 KiB, MDTS limit).
- Functional: 16-page bounce area (`DmaBuf::alloc(16)` + `authorize`) and one PRP-list page;
  PRP1 = first page, PRP2 = second page or the list address per NVMe §4.1.1.
- Non-functional: every PRP entry lies inside the authorized DMA range (host-tested).

## Architecture

`layout.rs` builds the PRP pair/list from the bounce area's page IOVAs; `controller.rs`
`submit_io` takes `(nlb, prp1, prp2)`; dispatch splits a request only at the max transfer.

## Assumptions

- **Claim:** `DmaBuf::alloc(16)` returns page-aligned, IOVA-contiguous memory.
  **Confidence:** medium. **Verify:** `DmaBuf` docs/impl and the `nvme-dma-addressing` lane.
- **Claim:** QEMU's NVMe reports MDTS ≥ 64 KiB (MDTS=7 with 4 KiB MPS → 512 KiB).
  **Confidence:** medium. **Verify:** log Identify at boot.

## Related Files

- Modify: `cells/drivers/nvme/src/{layout.rs,controller.rs,bounce.rs,dispatch.rs}`

## Implementation Steps

1. Identify: parse MDTS; log max transfer at init.
2. Bounce area 16 pages + PRP-list page; layout builds PRP1/PRP2/list; host tests: 1 page,
   2 pages (PRP2 direct), 16 pages (list), every entry inside the area, MDTS cap applied.
3. Dispatch: one command per ≤ max transfer.
4. Lanes: `nvme-dma-addressing`, `nvme-x86`; re-measure guest `dd`.

## Success Criteria

- [ ] `cargo test -p driver-nvme` passes with the new PRP tests.
- [ ] `nvme-dma-addressing` and `nvme-x86` lanes PASS.
- [ ] Guest `dd` 48 MiB measured and logged (expected faster than phase 1).

## Security Considerations

PRP entries are DMA addresses the device will write: built only from the driver's authorized
area; a host test asserts containment for every split.

## Risk Notes

If `DmaBuf` cannot give 16 contiguous authorized pages, keep per-page PRP list entries over
non-contiguous pages (the list does not need contiguity) and log the deviation.

## Deviation Log

- **Deviation (2026-11-12): dropped.** Phase 01 moved to inline `OP_READ_RUN` replies (see its
  log): a run is at most 7 sectors (3.5 KiB, the 4096 B IPC ceiling), which one PRP1 transfer
  into the existing bounce page already covers. A 64 KiB NVMe command has nothing to feed it
  until the VFS can take bulk data back through memory (needs a kernel Grant-policy change for
  VFS-owned Grants; not planned). `bounce.rs` now refuses a page that cannot hold a full run.
