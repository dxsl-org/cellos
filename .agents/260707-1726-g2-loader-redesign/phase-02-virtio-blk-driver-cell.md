---
phase: 02
title: virtio-blk Driver Cell (forbid(unsafe_code))
tier: thinking
status: pending
depends_on: [01]
---

# Phase 02 — virtio-blk Driver Cell

## Context links
- Plan: [plan.md](plan.md) · Scout: [scout-report.md](scout-report.md)
- Precedents: `cells/drivers/nvme/`, `cells/drivers/virtio-net/` (bounce-DMA CellHal), `cells/drivers/disk/src/lib.rs` (skeleton)
- Spec: `docs/specs/17-ipc-wire-contract.md` (DrvRequest framing, masked recv)

## Overview
**Priority:** core deliverable. Promote the userspace block driver into a real **virtio-blk Driver Cell** that owns the VirtIO block MMIO window + DMA, registers `service::BLOCK_DRIVER`, and serves read/write/flush over IPC — the same shape NVMe already ships. Cell is `#![forbid(unsafe_code)]`.

## Key insights
- The block-over-IPC contract already exists: VFS dual-routes to `service::BLOCK_DRIVER` (`block_stream.rs:42-84`). The Block Cell only has to *be* that service.
- DMA-from-cell-heap is solved: virtio-net's bounce-buffer `CellHal` (cell heap VA ≠ PA) is the template. Reuse it via ostd, not a fresh unsafe HAL.
- MMIO window is USER-mapped in the shared page table (per BS#1 / spec 15 §1.4). The cell acquires it via `sys_request_mmio` + `PcieDriverCap`-class grant (mirror the `/bin/virtio-net` grant path in `loader.rs:283-286`).
- IRQ: model on `vi_handle_virtio_irq` → move to the cell's `WaitIrq=234` path (as virtio-net did).

## Requirements
- **Functional:** Block Cell serves DrvRequest {Read(lba,count), Write(lba,buf), Flush} against the QEMU virtio-blk device; VFS reads/writes FAT32 (P1) + littlefs (P4) through it. Registers `service::BLOCK_DRIVER` on startup.
- **Non-functional:** `#![forbid(unsafe_code)]` (Law 4). Owned buffers across async (Law 2). Bounded reply size per spec 17 §5 — large reads use the grant path.

## F4 (red-team) — arch split for the block backend
- **RISC-V / aarch64 = virtio-blk over MMIO** (the 0x10001000 legacy window) — this cell is the primary deliverable and where validation happens **in this phase**.
- **x86_64 = pin to the existing NVMe Driver Cell** for disk I/O. Do NOT build a new x86 virtio-blk-pci path: modern VirtIO-PCI block (`0x1042`) was never implemented even in-kernel — the capability walk is deferred (`virtio_pci.rs:105-115`), so a cell version is net-new work, out of scope. x86's `/data`+`/mnt/sd` route to NVMe cell; virtio-blk cell is not required on x86.
- Consequence for Phase 06: removing kernel `virtio_pci::init()` on x86 is only safe once x86 block is served by the NVMe cell (see Phase 06 F4 rewrite).

## Architecture
- New cell `cells/drivers/virtio-blk/` (or promote `cells/drivers/disk/`). App-entry pattern `ostd::app_entry!` (not `#[no_mangle] main`).
- Uses `virtio-drivers` blk queue behind ostd's bounce-DMA HAL shim. MMIO base from `sys_request_mmio` (RISC-V 0x10001000 window; x86 via PCIe BAR grant).
- Request loop: masked `service_call`-style recv (spec 17 §2) → dispatch → reply via `try_send` (never blocking-send; spec 17 §6).
- Partition-range enforcement (`check_block_access` equivalent) stays authoritative in the **kernel** grant/cap layer — the cell serves sectors, the kernel gates which partitions a requesting cell may touch. (Do not move the security gate into the cell.)

## Related code files
- Create: `cells/drivers/virtio-blk/src/main.rs`, `Cargo.toml`, linker `.ld`, manifest (`block_io` + PcieDriver caps), syscall allowlist section.
- Modify: `loader.rs` (`/bin/block` → grant PcieDriverCap, like the other driver paths), service-id registration.
- Read-only ref: `cells/drivers/virtio-net/src/device.rs` (CellHal), `cells/drivers/nvme/`.

## Implementation steps
1. Scaffold the cell (copy virtio-net skeleton; strip net specifics).
2. Wire bounce-DMA HAL + virtio-blk queue init over the MMIO window.
3. Implement DrvRequest Read/Write/Flush with grant-based large transfers.
4. Register `service::BLOCK_DRIVER`; add `/bin/block` to bootstrap set (Phase 01 list) + loader grant path.
5. Add IRQ handling via `WaitIrq`.
6. Add integration test: Block Cell up → VFS read a known file → byte-exact match.

## Todo
- [ ] Cell scaffold + manifest/caps + `.ld`
- [ ] bounce-DMA HAL + queue init
- [ ] Read/Write/Flush over IPC + grant path
- [ ] `service::BLOCK_DRIVER` registration
- [ ] IRQ via WaitIrq
- [ ] Integration test: VFS-through-cell byte-exact

## Success criteria
- **Runtime evidence:** boot log shows Block Cell registered `BLOCK_DRIVER`; VFS reads/writes route to it (not the kernel syscall fallback); a FAT32 + a littlefs read return correct bytes. New integration test green on riscv64 (x86 PCIe path deferred to Phase 05 verify).

## Risk assessment
- *Bounce-DMA correctness* — reuse virtio-net's proven CellHal; add a round-trip write→read self-check at cell init.
- *IRQ ownership race with kernel* — kernel must stop claiming the blk IRQ once the cell owns it; coordinate with Phase 05 (kernel driver still present until then — guard against double-handling by keeping kernel virtio_blk IRQ disabled when `/bin/block` is live).

## Security considerations
- MMIO window USER-mapped: the cell is trusted first-party (BS#1). Partition-access gate stays in kernel. Cell holds `block_io` + PcieDriver caps only.

## Next steps
Phase 03 lets init spawn arbitrary cells by reading their ELF *through* this cell's VFS path.
