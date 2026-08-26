# Phase 01 — VirtIO-BLK Kernel Driver

**Status**: Planned
**Priority**: High — unblocks Phase 03 (and Phase 04)
**Parallel with**: Phase 02

---

## Context Links

- ADR: `docs/specs/09b-vfs-native-fs-adr.md`
- Existing block syscall: `kernel/src/task/syscall.rs` (BlkReadAsync, syscall 212)
- Existing RamDisk: `kernel/src/task/drivers/` (pattern to follow)
- `ViBlockDevice` trait: `libs/api/src/block.rs:10-26`
- `virtio-drivers` crate: `github.com/rcore-os/virtio-drivers` v0.13.0

---

## Overview

Add a `VirtIOBlk` kernel driver so `BlkReadAsync`/`BlkWriteAsync` route to a real VirtIO-BLK
device instead of the in-memory RamDisk. This is the block transport layer for Phase 03.

The driver lives in the **kernel** (not a Cell) because:
- `VirtIoHal` requires `unsafe impl` (DMA alloc, MMIO phys↔virt)
- Cells have `#![forbid(unsafe_code)]` — no exceptions (CLAUDE.md Law 4)
- Follows the existing RamDisk kernel-driver pattern

---

## Requirements

- Functional: QEMU `virt` machine (rv64/aarch64) reads/writes 512-byte sectors over VirtIO-BLK MMIO
- Functional: kernel probes VirtIO MMIO range at boot; registers device as active block device
- Functional: `BlkReadAsync` (212) routes to VirtIOBlk when device is present; falls back to RamDisk otherwise
- Non-functional: all `// SAFETY:` comments present; zero new clippy warnings

---

## Architecture

```
kernel boot
  └─ probe_virtio_devices()         (kernel/src/task/drivers/blk_virtio.rs)
       └─ VirtIOBlk::new(transport) (virtio-drivers crate)
            └─ VirtIoHalImpl        (hal/arch/riscv/src/virtio_hal.rs + arch/arm/)
                 ├─ dma_alloc  → FrameAllocator::alloc_dma_frames()
                 └─ mmio_phys_to_virt → |p| (p + HHDM_BASE) as NonNull<u8>
```

MMIO transport on `virt` machine: device base at `VIRTIO_MMIO_BASE` (from DTB or fixed
at `0x0A000000`; stride `0x200` per device). Magic `0x74726976` at offset 0 identifies
VirtIO MMIO. Device ID 2 = block device.

---

## Related Code Files

| File | Action |
|------|--------|
| `Cargo.toml` (workspace) | Add `virtio-drivers = { version = "0.13", default-features = false, features = ["blk"] }` |
| `hal/arch/riscv/src/virtio_hal.rs` | Create — `VirtIoHalImpl: Hal` unsafe impl (rv64 HHDM) |
| `hal/arch/arm/src/virtio_hal.rs` | Create — same pattern, aarch64 HHDM |
| `kernel/src/task/drivers/blk_virtio.rs` | Create — `VirtIoBlkDriver` wrapping `VirtIOBlk<VirtIoHalImpl, MmioTransport>` |
| `kernel/src/task/drivers/mod.rs` | Modify — expose `blk_virtio::probe()` + active-device selection |
| `kernel/src/task/syscall.rs` | Modify — route `BlkReadAsync` through `get_active_block_device()` |

---

## Implementation Steps

1. **Workspace dep**: add `virtio-drivers` with only `blk` feature enabled (keeps binary size minimal).

2. **`VirtIoHalImpl`** (one per arch, in `hal/arch/<arch>/src/virtio_hal.rs`):
   ```rust
   struct VirtIoHalImpl;
   // SAFETY: called from kernel init; single-owner DMA frames tracked in FrameAllocator.
   unsafe impl Hal for VirtIoHalImpl {
       unsafe fn dma_alloc(pages: usize, _dir: BufferDirection) -> (PhysAddr, NonNull<u8>) {
           let paddr = FRAME_ALLOCATOR.alloc(pages).expect("dma_alloc oom");
           let vaddr = paddr + HHDM_BASE;
           (paddr as PhysAddr, NonNull::new(vaddr as *mut u8).unwrap())
       }
       unsafe fn dma_dealloc(paddr: PhysAddr, _vaddr: NonNull<u8>, pages: usize) -> i32 {
           FRAME_ALLOCATOR.dealloc(paddr as usize, pages); 0
       }
       unsafe fn mmio_phys_to_virt(paddr: PhysAddr, _size: usize) -> NonNull<u8> {
           NonNull::new((paddr as usize + HHDM_BASE) as *mut u8).unwrap()
       }
       unsafe fn share(buffer: NonNull<[u8]>, _dir: BufferDirection) -> PhysAddr {
           (buffer.as_ptr() as *mut u8 as usize - HHDM_BASE) as PhysAddr
       }
       unsafe fn unshare(_paddr: PhysAddr, _buf: NonNull<[u8]>, _dir: BufferDirection) {}
   }
   ```

3. **`VirtIoBlkDriver`**: wrap `VirtIOBlk<VirtIoHalImpl, MmioTransport>`, implement `ViBlockDevice`.
   - `read_sector(sector, buf)` → `virtio_blk.read_blocks(sector as usize, buf)`
   - `write_sector(sector, buf)` → `virtio_blk.write_blocks(sector as usize, buf)`
   - `sector_count()` → `virtio_blk.capacity()`
   - `sector_size()` → 512 (VirtIO-BLK standard)

4. **Probe at boot**: scan MMIO range `[0x0A000000, 0x0A003E00)` for VirtIO magic. For each
   device with ID=2 (block), construct `MmioTransport`, init `VirtIOBlk`, register as active.
   Log `[virtio-blk] found: {capacity} sectors`.

5. **Active device registry** (`kernel/src/task/drivers/mod.rs`):
   ```rust
   static ACTIVE_BLK: Spinlock<Option<Box<dyn ViBlockDevice + Send + Sync>>> = ...;
   pub fn get_block_device() -> impl Deref<Target = dyn ViBlockDevice> { ... }
   pub fn register_block_device(dev: Box<dyn ViBlockDevice + Send + Sync>) { ... }
   ```
   `BlkReadAsync` syscall handler calls `get_block_device().read_sector(...)`.

6. **QEMU run scripts**: add `-device virtio-blk-device,drive=blk0` + matching `-drive`
   to `run.ps1` and `scripts/format-disk-arm.sh` for arm. Create `scripts/mksrv-img.sh`
   (mkfs.redoxfs wrapper, used by Phase 04).

---

## Todo

- [ ] Add `virtio-drivers` to workspace Cargo.toml
- [ ] Implement `VirtIoHalImpl` for riscv64 (`hal/arch/riscv/src/virtio_hal.rs`)
- [ ] Implement `VirtIoHalImpl` for aarch64 (`hal/arch/arm/src/virtio_hal.rs`)
- [ ] Create `VirtIoBlkDriver` in `kernel/src/task/drivers/blk_virtio.rs`
- [ ] Add active-device registry to `kernel/src/task/drivers/mod.rs`
- [ ] Route `BlkReadAsync` through registry
- [ ] Add VirtIO-BLK to QEMU run scripts (rv64 + aarch64)
- [ ] `cargo check -p vicell-kernel` passes for all three targets
- [ ] Manual QEMU boot: `[virtio-blk] found` log line visible

---

## Success Criteria

- `[virtio-blk] found: N sectors` appears in boot log on QEMU `virt` (rv64 and aarch64)
- `BlkReadAsync` syscall returns 1 (success) when invoked against sector 0 of the VirtIO disk
- `cargo clippy -p vicell-kernel` clean; all `// SAFETY:` comments present
- RamDisk still functions as fallback when no VirtIO-BLK device is present

---

## Risk

**MMIO probe at known base**: if DTB is available, parse it instead of hardcoding `0x0A000000`.
QEMU's `virt` machine is stable at that base, but future boards may differ.
Mitigation: add `VIRTIO_MMIO_BASE` as a hal constant per arch/board.

**VirtIO spec version**: `virtio-drivers` v0.13 targets VirtIO spec 1.1+. QEMU `virt` defaults to
legacy (0.9.5). Ensure QEMU args include `-device virtio-blk-device` (not `-device virtio-blk-pci`)
and that the crate falls back correctly to legacy negotiation if modern negotiation fails.
