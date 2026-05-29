# VirtIO Block Device — Root Cause & Fix (Phase 04)

**Date:** 2026-05-29 | **Severity:** P0 (blocked external ELF loading)

## Symptom

`VirtIOBlk::read_blocks()` spun forever after the kernel activated paging. Boot
never reached the PLIC init or driver init log lines when the disk image was
attached.

## Root Cause

`init_kernel_paging` (`kernel/src/memory/paging.rs`) maps only memory regions
reported by the Limine bootloader.  Limine does **not** include MMIO ranges in its
memory map.  After `activate_paging()`, every VirtIO MMIO register access at
`0x1000_1000+` caused a **load/store page fault → kernel panic**, which appeared
as a boot hang because the panic handler's SBI putchar also required an accessible
UART address.

Secondary root cause (keyboard): The VirtIO input device's `InterruptStatus`
register was never cleared by the kernel's IRQ handler.  After the first key press,
the PLIC re-fired the interrupt endlessly (interrupt storm), starving all polling
loops.

## Fix Applied

### 1. Explicit MMIO identity-mapping (primary fix)

`kernel/src/memory/paging.rs::init_kernel_paging` now maps three QEMU virt MMIO
ranges unconditionally **after** the bootloader-supplied mmap loop:

| Range | Size | Device |
|-------|------|--------|
| `0x0200_0000 – 0x0201_0000` | 64 KB | CLINT |
| `0x0C00_0000 – 0x1000_0000` | 64 MB | PLIC |
| `0x1000_0000 – 0x1001_0000` | 64 KB | UART + VirtIO (8 slots) |

`FALLBACK_MEMORY_MAP` (`boot.rs`) was updated to remove its duplicate MMIO entries
so there is a single source of truth.

### 2. VirtIO IRQ acknowledgement (secondary fix)

`vi_handle_virtio_irq` (`virtio_blk.rs`) now dispatches to **both** the block
device and input device.  Each calls `ack_interrupt()` to clear `InterruptStatus`
before the trap handler calls `plic_complete()`.

### 3. DMA alignment audit

`VirtioHal::dma_alloc` already returns 4096-byte aligned buffers with
`PAddr == VAddr` (identity mapping).  No change needed.

### 4. Defensive poll-warn threshold

`read_sector` / `write_sector` now log a warning if the spin count exceeds
`POLL_WARN_THRESHOLD = 10_000_000`, distinguishing a live-but-slow device from a
complete hang.

## Files Changed

- `kernel/src/memory/paging.rs` — explicit MMIO mappings
- `kernel/src/boot.rs` — removed duplicate MMIO from FALLBACK_MEMORY_MAP
- `kernel/src/task/drivers/virtio_blk.rs` — IRQ dispatch + poll fallback + SAFETY
- `kernel/src/task/drivers/virtio_hal.rs` — SAFETY comments
- `kernel/src/task/drivers/virtio_input.rs` — INPUT_DEVICE_IRQ + ack_irq()

## Test Added

`tests/integration/virtio_block.rs` — smoke test (LBA 0 read) + soak test (1000
reads).  Gated on `feature = "virtio-block-test"`; meant to be enabled in CI once
Phase 06 wires disk image into the QEMU launch command.

## Reuse Note for Phase 15 (NIC) and Phase 16 (GPU)

Every new VirtIO device needs:
1. Its IRQ slot recorded in a `static DEVICE_IRQ: Spinlock<u32>`
2. An `ack_irq(irq: u32) -> bool` function
3. A dispatch call added to `vi_handle_virtio_irq` in `virtio_blk.rs`
4. Its MMIO address range added to the explicit block in `init_kernel_paging`
