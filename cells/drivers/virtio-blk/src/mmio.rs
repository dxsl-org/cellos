//! VirtIO-MMIO discovery and DMA HAL (riscv64 / aarch64).
//!
//! # Unsafe island
//! `unsafe` is confined here (and to the x86_64 counterparts) for two reasons:
//!   1. `virtio_drivers::Hal` requires `unsafe impl` (the trait is `unsafe trait`).
//!   2. `MmioTransport::new` takes a raw header pointer to the claimed slot.
//!
//! Slots are identified by the kernel (`sys_probe_virtio_mmio`): the VirtIO
//! window is kernel-only until a claim grants it.

#![allow(unsafe_code)]

use crate::device::BlkDevice;
use core::ptr::NonNull;
use ostd::syscall::{sys_grant_alloc, sys_grant_free, sys_probe_virtio_mmio, BlockDevice};
use virtio_drivers::{
    device::blk::VirtIOBlk,
    transport::mmio::{MmioTransport, VirtIOHeader},
    transport::{DeviceType, Transport},
    BufferDirection, Hal, PhysAddr,
};

/// Transport driving the device on this architecture.
pub(crate) type CellTransport = MmioTransport;

// ─── Constants ───────────────────────────────────────────────────────────────

/// VirtIO device type: Block (2).
const VIRTIO_DEV_BLK: u16 = 2;

/// Window claimed per slot. RISC-V spaces slots a page apart and the kernel
/// registers each as its page, so the claim maps it for U-mode; AArch64 packs
/// eight 0x200 slots per page, whose accesses the kernel mediates.
#[cfg(target_arch = "riscv64")]
pub const MMIO_SLOT_SIZE: usize = 0x1000;
#[cfg(not(target_arch = "riscv64"))]
pub const MMIO_SLOT_SIZE: usize = 0x200;

// ─── CellHal ─────────────────────────────────────────────────────────────────

/// VirtIO DMA HAL backed by `sys_grant_alloc` / `sys_grant_free`.
///
/// In Cellos SAS, physical address == virtual address for grant-allocated pages
/// (identity mapping), so `phys == virt == grant_id`. Cell image memory
/// (heap/.bss/stack) lives at loader VAs that are NOT identity-mapped, so DMA
/// buffers are bounced through identity-mapped grant pages (see `share`).
/// Identical to the virtio-net cell's CellHal.
///
/// # Abandoned requests
/// A request that misses its deadline is never completed (`crate::io`), so
/// `unshare` is never reached for it: its request, data and response bounce
/// grants deliberately stay allocated and device-addressable until cell
/// teardown, and a late completion lands there, not in cell memory. Poisoning
/// refuses every later request, so at most one request's grants are lost.
pub(crate) struct CellHal;

// SAFETY: CellHal is a zero-sized stateless type; all ops go through kernel syscalls.
unsafe impl Hal for CellHal {
    fn dma_alloc(pages: usize, _dir: BufferDirection) -> (PhysAddr, NonNull<u8>) {
        let base = sys_grant_alloc(pages * 4096).expect("[virtio-blk] DMA OOM");
        let ptr = NonNull::new(base as *mut u8).expect("[virtio-blk] DMA grant at null");
        (base, ptr)
    }

    unsafe fn dma_dealloc(paddr: PhysAddr, _vaddr: NonNull<u8>, _pages: usize) -> i32 {
        // paddr == grant_id (SAS identity mapping)
        sys_grant_free(paddr);
        0
    }

    unsafe fn mmio_phys_to_virt(paddr: PhysAddr, _size: usize) -> NonNull<u8> {
        // SAS identity mapping: paddr is a kernel-mapped MMIO address.
        NonNull::new(paddr as *mut u8).expect("[virtio-blk] MMIO address is null")
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
                core::ptr::copy_nonoverlapping(
                    buffer.as_ptr() as *const u8,
                    bounce as *mut u8,
                    len,
                );
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

/// Discovery rounds (one yield apart) while a free-looking block slot is still
/// held, before concluding none is free: a restarted cell can probe before its
/// predecessor's claim is reaped. The bound the PCI driver cells use.
const FIND_ATTEMPTS: usize = 200;

/// Probe all VirtIO MMIO slots and initialise the first claimable Block device.
///
/// A block slot another Cell still holds is retried, a yield apart, for at
/// most [`FIND_ATTEMPTS`] rounds; otherwise the scan ends after one round.
/// Returns `None` when no claimable block device is present (graceful exit).
pub fn find_and_init_blk() -> Option<BlkDevice> {
    for _ in 0..FIND_ATTEMPTS {
        let mut held = false;
        if let Some(device) = init_first_free(&mut held) {
            return Some(device);
        }
        if !held {
            break;
        }
        ostd::task::yield_now();
    }
    None
}

/// One pass over the slots; sets `held` when a block slot's claim failed.
fn init_first_free(held: &mut bool) -> Option<BlkDevice> {
    for (base, _irq) in virtio_slot_iter() {
        let Ok(identity) = sys_probe_virtio_mmio(base) else {
            continue;
        };
        if identity.device_id != VIRTIO_DEV_BLK {
            continue;
        }

        // Claim exclusive MMIO ownership via the kernel resource registry.
        if ostd::mmio::request_region(base, MMIO_SLOT_SIZE).is_err() {
            // Owned by another Cell, live or awaiting reap.
            *held = true;
            continue;
        }

        // Create the VirtIO transport.
        let Some(header) = NonNull::new(base as *mut VirtIOHeader) else {
            continue;
        };
        // SAFETY: the kernel identified a VirtIO block transport at `base` and
        // granted this Cell the slot above, so the header is live device memory
        // this Cell may access.
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

        return Some(BlkDevice {
            blk,
            route: BlockDevice::Mmio { base: base as u64 },
            poisoned: Default::default(),
        });
    }
    None
}
