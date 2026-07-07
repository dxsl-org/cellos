//! VirtIO-MMIO block device layer for the virtio-blk Driver Cell.
//!
//! # Unsafe island
//! This module is the ONLY place in the cell with `unsafe` code, for two reasons:
//!   1. `virtio_drivers::Hal` requires `unsafe impl` (the trait is `unsafe trait`).
//!   2. MMIO probe reads (`read_volatile` of magic / device-id / status) must
//!      happen before we claim the slot.
//!
//! All other modules in this cell are `#![forbid(unsafe_code)]`.

#![allow(unsafe_code)]

extern crate alloc;

use core::ptr::NonNull;
use virtio_drivers::{
    device::blk::VirtIOBlk,
    transport::mmio::{MmioTransport, VirtIOHeader},
    transport::{DeviceType, Transport},
    BufferDirection, Hal, PhysAddr,
};
use ostd::syscall::{sys_grant_alloc, sys_grant_free};

// ─── Constants ───────────────────────────────────────────────────────────────

/// VirtIO MMIO magic value ("virt" in little-endian ASCII).
const VIRTIO_MAGIC: u32 = 0x7472_6976;

/// VirtIO device type: Block (2).
const VIRTIO_DEV_BLK: u32 = 2;

/// VirtIO MMIO register area size per slot — 0x200 on both QEMU virt boards.
pub const MMIO_SLOT_SIZE: usize = 0x200;

/// VirtIO MMIO `Status` register offset (virtio-mmio spec §4.2.2).
const VIRTIO_STATUS_OFFSET: usize = 0x070;

/// `DRIVER_OK` status bit. When set, another driver (the in-kernel virtio_blk)
/// has already brought this device up — we must NOT touch it.
const VIRTIO_STATUS_DRIVER_OK: u32 = 0x04;

// ─── CellHal ─────────────────────────────────────────────────────────────────

/// VirtIO DMA HAL backed by `sys_grant_alloc` / `sys_grant_free`.
///
/// In Cellos SAS, physical address == virtual address for grant-allocated pages
/// (identity mapping), so `phys == virt == grant_id`. Cell image memory
/// (heap/.bss/stack) lives at loader VAs that are NOT identity-mapped, so DMA
/// buffers are bounced through identity-mapped grant pages (see `share`).
/// Identical to the virtio-net cell's CellHal.
pub(crate) struct CellHal;

// SAFETY: CellHal is a zero-sized stateless type; all ops go through kernel syscalls.
unsafe impl Hal for CellHal {
    fn dma_alloc(pages: usize, _dir: BufferDirection) -> (PhysAddr, NonNull<u8>) {
        let base = sys_grant_alloc(pages * 4096).expect("[virtio-blk] DMA OOM");
        // SAFETY: base is a non-null page-aligned address from the kernel frame allocator.
        (base, unsafe { NonNull::new_unchecked(base as *mut u8) })
    }

    unsafe fn dma_dealloc(paddr: PhysAddr, _vaddr: NonNull<u8>, _pages: usize) -> i32 {
        // paddr == grant_id (SAS identity mapping)
        sys_grant_free(paddr);
        0
    }

    unsafe fn mmio_phys_to_virt(paddr: PhysAddr, _size: usize) -> NonNull<u8> {
        // SAFETY: SAS identity mapping — paddr is a valid MMIO address pre-mapped by the kernel.
        unsafe { NonNull::new_unchecked(paddr as *mut u8) }
    }

    unsafe fn share(buffer: NonNull<[u8]>, dir: BufferDirection) -> PhysAddr {
        // Bounce through an identity-mapped grant page: cell heap/.bss/stack VAs are
        // NOT identity-mapped, so the device cannot DMA there. Grant pages satisfy
        // vaddr == paddr, so the returned base is both.
        let len = buffer.len();
        let bounce = sys_grant_alloc(len).expect("[virtio-blk] bounce OOM");
        if matches!(dir, BufferDirection::DriverToDevice | BufferDirection::Both) {
            // SAFETY: buffer is a live slice owned by virtio-drivers for the DMA
            // duration; bounce is a fresh grant allocation of >= len bytes.
            unsafe {
                core::ptr::copy_nonoverlapping(buffer.as_ptr() as *const u8, bounce as *mut u8, len);
            }
        }
        bounce as PhysAddr
    }

    unsafe fn unshare(paddr: PhysAddr, buffer: NonNull<[u8]>, dir: BufferDirection) {
        // Copy device-written bytes back into the driver's buffer, then release the
        // bounce page. paddr == grant base (see share()).
        if matches!(dir, BufferDirection::DeviceToDriver | BufferDirection::Both) {
            let len = buffer.len();
            // SAFETY: paddr is the grant page returned by share() (still mapped);
            // buffer is the same slice passed to share(), valid for len bytes.
            unsafe {
                core::ptr::copy_nonoverlapping(paddr as *const u8, buffer.as_ptr() as *mut u8, len);
            }
        }
        sys_grant_free(paddr);
    }
}

// ─── Device state ─────────────────────────────────────────────────────────────

pub(crate) type CellBlk = VirtIOBlk<CellHal, MmioTransport>;

/// Runtime state for the active VirtIO block device.
pub struct BlkDevice {
    blk: CellBlk,
}

impl BlkDevice {
    /// Read one 512-byte sector. Returns `true` on success.
    pub fn read_sector(&mut self, sector: u64, buf: &mut [u8]) -> bool {
        self.blk.read_blocks(sector as usize, buf).is_ok()
    }

    /// Write one 512-byte sector. Returns `true` on success.
    pub fn write_sector(&mut self, sector: u64, buf: &[u8]) -> bool {
        self.blk.write_blocks(sector as usize, buf).is_ok()
    }
}

// ─── Slot iterator ────────────────────────────────────────────────────────────

/// Yields `(mmio_base, irq)` for each VirtIO MMIO slot on the current platform.
///
/// AArch64 QEMU virt: 32 slots at 0x0a000000, stride 0x200, SPI 16+i.
/// RISC-V  QEMU virt:  8 slots at 0x10001000, stride 0x1000, IRQ 1+i.
fn virtio_slot_iter() -> impl Iterator<Item = (usize, u32)> {
    #[cfg(target_arch = "aarch64")]
    {
        const BASE: usize = 0x0a00_0000;
        const STRIDE: usize = 0x200;
        (0..32_usize).map(|i| (BASE + i * STRIDE, 16 + i as u32))
    }
    #[cfg(target_arch = "riscv64")]
    {
        const BASE: usize = 0x1000_1000;
        const STRIDE: usize = 0x1000;
        (0..8_usize).map(|i| (BASE + i * STRIDE, 1 + i as u32))
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "riscv64")))]
    {
        core::iter::empty()
    }
}

// ─── Device init ──────────────────────────────────────────────────────────────

/// Probe all VirtIO MMIO slots and initialise the first FREE Block device.
///
/// A block device already showing `DRIVER_OK` is owned by the in-kernel
/// virtio_blk driver (boot disk) — it is skipped so this cell never resets the
/// live device during the migration window (phase 02). Returns `None` when no
/// free block device is present (graceful exit).
pub fn find_and_init_blk() -> Option<BlkDevice> {
    for (base, _irq) in virtio_slot_iter() {
        // Probe magic directly (U-mode, identity-mapped MMIO window).
        // SAFETY: base is within the arch MMIO window mapped USER-accessible by init_kernel_paging.
        let magic = unsafe { core::ptr::read_volatile(base as *const u32) };
        if magic != VIRTIO_MAGIC {
            continue;
        }

        // Device type at offset 8.
        // SAFETY: same invariant; base + 8 is within the same 0x200-byte slot.
        let device_id = unsafe { core::ptr::read_volatile((base + 8) as *const u32) };
        if device_id != VIRTIO_DEV_BLK {
            continue;
        }

        // Coexistence guard: skip a block device the kernel already owns
        // (DRIVER_OK set). Creating a transport on it would reset the live boot
        // disk. Reading Status is a plain register read — it does not disturb the
        // device. Once the kernel relinquishes the device (phase 06), Status is
        // clear here and the claim proceeds.
        // SAFETY: same MMIO window; Status register at a fixed offset.
        let status = unsafe { core::ptr::read_volatile((base + VIRTIO_STATUS_OFFSET) as *const u32) };
        if status & VIRTIO_STATUS_DRIVER_OK != 0 {
            continue;
        }

        // Claim exclusive MMIO ownership via the kernel resource registry.
        if ostd::mmio::request_region(base, MMIO_SLOT_SIZE).is_err() {
            continue;
        }

        // Create the VirtIO transport.
        // SAFETY: base was validated (magic + type) and claimed above; it is a live
        // VirtIO MMIO header within the USER-accessible identity-mapped window.
        let header = unsafe { NonNull::new_unchecked(base as *mut VirtIOHeader) };
        let transport = match unsafe { MmioTransport::new(header) } {
            Ok(t) if t.device_type() == DeviceType::Block => t,
            Ok(t) => {
                // Type changed between probe and transport init (race) — forget to
                // avoid resetting a slot owned by another driver, then move on.
                core::mem::forget(t);
                continue;
            }
            Err(_) => continue,
        };

        let blk = match VirtIOBlk::<CellHal, MmioTransport>::new(transport) {
            Ok(b) => b,
            Err(_) => continue,
        };

        return Some(BlkDevice { blk });
    }
    None
}
