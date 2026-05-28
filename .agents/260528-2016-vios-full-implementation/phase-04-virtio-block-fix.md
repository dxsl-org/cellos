# Phase 04 — VirtIO Block Device Fix

**Effort:** 40h | **Priority:** P0 (BLOCKING) | **Status:** pending | **Blockers:** none

## Overview

The current VirtIO block device driver hangs during read/write. RamDisk is in use as a workaround which blocks Phase 06 (external ELF loading from disk). Goal: deterministic, sub-100ms 4KB read/write from a VirtIO MMIO block device on `qemu-system-riscv64 -machine virt`.

## Context Links

- `docs/02-memory.md` — DMA buffer alignment expectations
- `docs/04-hardware.md` — driver registration model
- `kernel/src/task/drivers/virtio_blk.rs` — current driver source
- `kernel/src/task/drivers/virtio_hal.rs` — VirtIO HAL bridge
- `kernel/src/task/drivers/ramdisk.rs` — current workaround to be retired (post Phase 06)

## Key Insights

- VirtIO 1.1 spec: virtqueue descriptor table, available ring, and used ring all 16-byte aligned; descriptor table must be physically contiguous and HHDM-addressable.
- For RV64 QEMU virt: VirtIO MMIO device discovery walks the device tree (or hard-coded base `0x10001000+`). Each MMIO slot is 0x1000.
- DMA buffers: kernel must hand the device a *physical* address; with our SAS/HHDM design, `PAddr = VAddr - HHDM_OFFSET` once HHDM is set up.
- Interrupt path: VirtIO device fires IRQ via PLIC; driver must claim (`PLIC_CLAIM`), service used ring, then complete (`PLIC_COMPLETE`). Missing complete keeps the interrupt latched and second I/O hangs.
- `virtio-drivers = 0.7.x` crate provides the heavy lifting; bug usually lives in our `VirtIoHal` adapter (DMA alloc returning wrong PAddr or non-aligned buffer).

## Requirements

**Functional**
- Driver enumerates the VirtIO block device on boot
- `read_block(lba, &mut buf[0..512])` completes within 100ms in QEMU
- `write_block(lba, &buf[0..512])` round-trips successfully (read-back matches)
- 1000 sequential 4KB reads complete without hang
- Concurrent read + interrupt-driven completion works without deadlock

**Non-functional**
- Driver compiled with `#![forbid(unsafe_code)]` at *cell* level (this driver lives in kernel space; `unsafe` allowed but each block annotated)
- Zero allocations in the I/O hot-path beyond the per-request descriptor pin

## Architecture

```
Caller (kernel or service)
   │ block_read(lba, buf) [async or sync]
   ▼
virtio_blk::BlockDevice
   ├─ pick free descriptor pair from virtqueue
   ├─ build VIRTIO_BLK_T_IN request header
   ├─ chain: [hdr (R)] → [buf (W)] → [status (W)]
   ├─ publish to available ring (head index)
   ├─ memory barrier (fence rw,rw)
   ├─ notify device (write to queue_notify MMIO reg)
   └─ wait completion (poll used ring or sleep on waker)
                 ▲
                 │ IRQ via PLIC
            trap::handle_irq → virtio_blk::on_irq → wake waker
```

## Related Code Files

**Investigate first:**
- `kernel/src/task/drivers/virtio_blk.rs` — current hanging driver
- `kernel/src/task/drivers/virtio_hal.rs` — DMA alloc/free, MMIO accessor
- `kernel/src/task/drivers/registry.rs` — driver discovery + registration
- `kernel/src/task/drivers.rs` — driver subsystem entrypoint

**Modify:**
- `kernel/src/task/drivers/virtio_blk.rs` — fix queue management + IRQ servicing
- `kernel/src/task/drivers/virtio_hal.rs` — fix DMA-to-phys conversion
- `cells/drivers/disk/src/lib.rs` — eventual user-space driver wrapper (kernel-side driver still authoritative for now)
- `kernel/src/task/drivers/registry.rs` — ensure VirtIO block is registered before VFS comes up
- `libs/api/src/block.rs` — confirm `ViBlockDevice` trait matches actual implementation

**Create:**
- `scripts/qemu-virtio-trace.sh` — wrapper that runs QEMU with `-trace virtio_*,file=qemu-virtio.log`
- `tests/integration/virtio_block.rs` — boot, read LBA 0, assert MBR-like signature or known bytes
- `kernel/src/task/drivers/virtio_blk_internal_notes.md` — short note documenting the discovered root cause + the fix (committed for future debuggers)

**Files affected indirectly (no source edit, but build implications):**
- `kernel/Cargo.toml` — confirm `virtio-drivers = "0.7"` features (`alloc`, `mmio`)
- `kernel/build.rs` — may need to expose virtio MMIO base via env var if currently hardcoded

## Implementation Steps

1. **Reproduce the hang deterministically**:
   - Run `bash scripts/qemu-virtio-trace.sh` (creates `qemu-virtio.log`)
   - Inspect the last few `virtio_queue_notify` / `virtio_blk_handle_request` lines; identify whether device received notify (driver-side bug) or never responded (device-side / IRQ bug)
2. **Audit `virtio_hal.rs` DMA alloc**:
   - `dma_alloc` must return a frame from the global allocator AND its PAddr
   - Confirm: `paddr = vaddr - HHDM_OFFSET` (where vaddr came from `vmm::alloc_frame()`)
   - Verify allocation is page-aligned (4096); virtqueue prefers ≥4KB alignment.
3. **Audit virtqueue setup**:
   - Descriptor table, available ring, used ring all in one DMA-allocated, page-aligned chunk
   - Sizes: desc table = 16*queue_size, avail = 6+2*queue_size, used = 6+8*queue_size, padded to next page
   - Write each base PAddr into the corresponding MMIO regs (`QueueDescLow/High`, `QueueDriverLow/High`, `QueueDeviceLow/High`)
   - Set `QueueReady = 1`
4. **Audit notify path**:
   - After publishing the head index, issue `fence rw, rw`
   - Read `used_event` from avail ring; if device opted in to notification suppression, skip notify (otherwise always notify)
   - Write the queue index to `QueueNotify` MMIO reg
5. **Audit IRQ path**:
   - Confirm PLIC entry for the VirtIO block IRQ is enabled in `hal/arch/riscv/src/rv64/trap.rs` PLIC init
   - In trap handler, read PLIC claim register → if IRQ matches VirtIO block → call `virtio_blk::on_irq()` → write PLIC complete
   - `on_irq` reads used-ring index, drains completions, wakes wakers
6. **Add poll fallback for first 100ms** (defensive):
   - If IRQ never fires within 100ms, busy-poll used ring once and log warning
   - Helps distinguish IRQ-routing bug from device hang during debug
7. **Smoke-test 4KB read of LBA 0**:
   - Replace RamDisk content with a real raw disk image (`gen_disk.ps1` already builds one)
   - Boot kernel, log first 16 bytes read from LBA 0
   - Expect MBR/GPT/RedoxFS signature
8. **Soak test**:
   - Loop 1000 reads of LBA 0 in `tests/integration/virtio_block.rs`
   - Then 1000 random reads (use fixed PRNG seed for determinism)
   - Assert no hang, no panic, all reads return expected bytes
9. **Write path**:
   - Implement `write_block`; round-trip test (write, then read, assert match)
   - Validate the write actually persisted: shut down QEMU, re-boot, read same LBA, expect same data (only works if disk image is shared persistent file, not `-snapshot`)
10. **Document fix** in `virtio_blk_internal_notes.md` (one short page: symptom, root cause, fix, test added).

## Todo List

- [ ] Reproduce hang with `qemu-virtio-trace.sh`
- [ ] Identify root cause from trace (driver vs IRQ vs device)
- [ ] Fix `virtio_hal.rs` DMA alloc / paddr conversion
- [ ] Fix virtqueue setup (alignment, MMIO reg writes)
- [ ] Fix notify path (fence + queue_notify)
- [ ] Fix IRQ path (PLIC claim/complete)
- [ ] Add poll fallback for first 100ms (defense)
- [ ] Smoke-test single 4KB read
- [ ] Soak-test 1000 sequential + 1000 random reads
- [ ] Implement + test write_block
- [ ] Persistence check across reboot
- [ ] Document fix in internal notes
- [ ] CI integration test green

## Success Criteria

- 4KB read of LBA 0 completes < 100ms in QEMU
- 1000 sequential reads + 1000 random reads pass without hang/panic
- Write → reboot → read round-trip equal
- `tests/integration/virtio_block.rs` passes in CI
- No regression on Ring 3 smoke test from Phase 03

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| HHDM not active yet when driver inits | Med | High | Order driver init AFTER paging init in `kernel/src/main.rs`; add assertion |
| `virtio-drivers` crate API changed between 0.5 and 0.7 | High | Med | Pin exact version in `kernel/Cargo.toml`; document in lock file |
| QEMU virtio MMIO base differs across versions | Med | Med | Read device tree at boot (Limine provides FDT) rather than hardcoding |
| Phase 05 keyboard fix touches same trap handler — merge conflict | High | Low | Coordinate file boundaries: virtio_blk owns the IRQ dispatch entry, keyboard owns the input IRQ entry; both register via `registry.rs` |
| Real disk image format mismatch (RedoxFS vs raw) | Low | Low | Phase 04 only validates raw block I/O; FS layer is Phase 13 |

## Security Considerations

- DMA buffer must NOT alias kernel data structures — use dedicated frames from allocator
- Validate `used.ring[].len` ≤ submitted buffer size before trusting it (defense vs malicious device firmware, also catches bugs)
- The VirtIO device is fully trusted in QEMU; on real hardware (post v1.0) revisit threat model in Phase 12

## Rollback

If driver fix destabilizes the kernel, revert the driver PR; RamDisk continues to satisfy boot needs until Phase 06 forces external disk loading. Phase 06 explicitly depends on this phase being green.

## Next Steps

Unblocks: Phase 06 (external ELF loading), Phase 13 (full VFS), Phase 15 (NIC driver mirrors VirtIO patterns), Phase 16 (VirtIO GPU). Document the resolved VirtIO + IRQ patterns in `virtio_blk_internal_notes.md` so Phase 15/16 reuse them.
