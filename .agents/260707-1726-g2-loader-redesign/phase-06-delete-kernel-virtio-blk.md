---
phase: 06
title: Delete kernel virtio_blk + virtio_pci stack
tier: medium
status: pending
depends_on: [05]
---

# Phase 06 — Delete the kernel virtio_blk stack

## Context links
- Plan: [plan.md](plan.md) · Scout: [scout-report.md](scout-report.md)
- Spec update target: `docs/specs/15-kernel-boundary.md §2C` + `§3.1` (Phase 08 does the edit)

## Overview
With all runtime block reads routed off the kernel (Phase 05), physically remove the block-only driver code: `virtio_blk.rs` (217) + `virtio_pci.rs` (225, block-only transport). **Keep** `virtio_hal.rs` + `virtio_common.rs` — shared with `virtio_rng`.

## Key insights
- Scout confirmed `virtio_pci.rs` is BLK-only now; `virtio_hal`/`virtio_common` are shared → **must not** be deleted (verify `virtio_rng`/entropy still builds).
- **F4 (red-team) — x86 is not free here.** x86 boot still calls `virtio_pci::init()` (`main.rs:414`) and x86 VFS data I/O (`/data`, `/mnt/sd`) currently uses `sys_blk_read → block::read_sector → virtio_pci` (`block_stream.rs:63-83`). Removing `virtio_pci::init()` is a no-op for *cell loading* (x86 loads from VIFS1, `main.rs:438`) but **breaks x86 data-partition I/O unless x86 block is already served by the NVMe cell** (Phase 02 F4). Verify NVMe-cell-backed `/data` on x86 is green before deleting.
- `block.rs` dispatch loses its VirtIO arm; MMC remains (descoped G2, real-board only).

## Requirements
- **Functional:** kernel compiles + boots 3-arch with `virtio_blk.rs`/`virtio_pci.rs` removed; block I/O works entirely via Driver Cells (virtio-blk cell RISC-V/ARM, NVMe cell x86).
- **Non-functional:** `grep -r 'virtio_blk\|virtio_pci' kernel/src` returns only comments/history, no live modules.

## Architecture
- Delete `virtio_blk.rs`, `virtio_pci.rs`, their `mod` decls, and `#[no_mangle] vi_handle_virtio_irq` (moved to the cell in Phase 02).
- `block.rs`: drop the VirtIO arm (MMC-only or remove wrappers if no kernel caller remains after Phase 05).
- x86: remove `virtio_pci::init()` from `main.rs:414` **after** confirming NVMe-cell `/data` is green.

## Related code files
- Delete: `kernel/src/task/drivers/virtio_blk.rs`, `kernel/src/task/drivers/virtio_pci.rs`.
- Modify: `kernel/src/task/drivers.rs`/`main.rs` (remove mod + init calls), `block.rs` (drop VirtIO arm), any `store_pci_device` refs.
- Keep: `virtio_hal.rs`, `virtio_common.rs`.

## Implementation steps
1. Confirm x86 NVMe-cell `/data`+`/mnt/sd` I/O green (F4 gate).
2. Remove `virtio_pci::init()` from x86 boot; x86 still reaches shell + data I/O.
3. Delete `virtio_blk.rs` + `virtio_pci.rs` + mod decls + IRQ handler.
4. Fix `block.rs` dispatch (MMC-only / removed wrappers).
5. `cargo build` 3-arch; resolve residual references; verify `virtio_rng` builds.
6. Boot 3-arch to shell; VFS read/write via Driver Cells.

## Todo
- [ ] x86 NVMe-cell data I/O green (F4 gate passed)
- [ ] x86 `virtio_pci::init` removed, x86 boots + data I/O
- [ ] `virtio_blk.rs` + `virtio_pci.rs` deleted
- [ ] block.rs dispatch fixed
- [ ] 3-arch build clean, `virtio_rng` intact, no live virtio_blk refs
- [ ] 3-arch boot to shell + VFS I/O via cells

## Success criteria
- **Runtime evidence:** 3-arch boot to shell with the driver files deleted; VFS FAT32 + littlefs I/O correct through Driver Cells; `grep` proof of no live modules. Concrete RC-4 closure. (Docs/memory update is Phase 08.)

## Risk assessment
- *x86 data I/O breakage (F4)* — step 1 gate; do not delete until NVMe-cell `/data` proven on x86.
- *Shared HAL accidentally removed* — explicit keep-list; verify entropy/`virtio_rng`.

## Security considerations
- Kernel no longer contains DMA-programming block driver code → block's IOMMU/USER-MMIO surface (BS#1) moves to trusted-first-party cells, consistent with net/gpu/nvme.

## Next steps
Phase 07 (SUM, deferred/spike-first) or straight to Phase 08 regression + docs if SUM is split out.
