//! VirtIO MMIO input device layer for the Input Service Cell.
//!
//! # Unsafe island
//! This module is the ONLY place with `unsafe` code in the cell.
//!   1. MMIO probe reads (`read_volatile`) to detect magic + device type.
//!   2. `virtio_drivers::Hal` requires `unsafe impl`.
//!
//! All other modules in this cell are unsafe-free.

#![allow(unsafe_code)]

extern crate alloc;

use core::ptr::NonNull;
use virtio_drivers::{
    BufferDirection, Hal, PhysAddr,
    device::input::VirtIOInput,
    transport::mmio::{MmioTransport, VirtIOHeader},
    transport::{DeviceType, Transport},
};
use ostd::syscall::{sys_grant_alloc, sys_grant_free, sys_request_mmio};

// ─── Constants ───────────────────────────────────────────────────────────────

const VIRTIO_MAGIC: u32   = 0x7472_6976;
const VIRTIO_DEV_INPUT: u32 = 18; // VirtIO device type: Input (18)
pub const MMIO_SLOT_SIZE: usize = 0x200;

// ─── CellHal ─────────────────────────────────────────────────────────────────

/// VirtIO DMA HAL — SAS identity mapping (phys == virt == grant_id).
pub(crate) struct CellHal;

// SAFETY: zero-sized stateless type; all ops go through kernel syscalls.
unsafe impl Hal for CellHal {
    fn dma_alloc(pages: usize, _dir: BufferDirection) -> (PhysAddr, NonNull<u8>) {
        let base = sys_grant_alloc(pages * 4096).expect("[input] DMA OOM");
        // SAFETY: kernel-allocated non-null page-aligned address.
        (base, unsafe { NonNull::new_unchecked(base as *mut u8) })
    }

    unsafe fn dma_dealloc(paddr: PhysAddr, _vaddr: NonNull<u8>, _pages: usize) -> i32 {
        sys_grant_free(paddr);
        0
    }

    unsafe fn mmio_phys_to_virt(paddr: PhysAddr, _size: usize) -> NonNull<u8> {
        // SAFETY: SAS identity mapping.
        unsafe { NonNull::new_unchecked(paddr as *mut u8) }
    }

    unsafe fn share(buffer: NonNull<[u8]>, dir: BufferDirection) -> PhysAddr {
        // The VirtIOInput event buffers live in a `Box<[InputEvent; 32]>` on the
        // cell heap, whose loader VA is NOT identity-mapped — the device cannot
        // DMA there (it would write events to a bogus physical address and the
        // driver never sees them). Bounce through an identity-mapped grant page
        // (grant VAs satisfy vaddr == paddr), mirroring the virtio-net cell.
        let len = buffer.len();
        let bounce = sys_grant_alloc(len).expect("[input] bounce OOM");
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
        // Copy device-written event bytes back into the driver's buffer, then
        // release the bounce page. paddr == grant base (see share()).
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

// ─── Raw event from virtqueue ─────────────────────────────────────────────────

/// One raw VirtIO input event as drained from the virtqueue.
pub struct RawEvent {
    pub event_type: u16,
    pub code:       u16,
    pub value:      u32,
}

// ─── Device state ─────────────────────────────────────────────────────────────

type InputDev = VirtIOInput<CellHal, MmioTransport>;

pub struct InputDevice {
    dev:      InputDev,
    pub irq:  u32,
    pub base: usize,
}

impl InputDevice {
    /// Drain one pending event from the virtqueue. Returns `None` when the queue
    /// is empty.
    pub fn try_get_event(&mut self) -> Option<RawEvent> {
        self.dev.pop_pending_event().map(|ev| RawEvent {
            event_type: ev.event_type,
            code:       ev.code,
            value:      ev.value,
        })
    }

    /// Acknowledge the VirtIO interrupt (must call after waking on IRQ).
    pub fn ack_irq(&mut self) {
        self.dev.ack_interrupt();
    }
}

// ─── Slot iterator ────────────────────────────────────────────────────────────

/// Yields `(mmio_base, irq)` for each VirtIO MMIO slot on the current platform.
fn virtio_slot_iter() -> impl Iterator<Item = (usize, u32)> {
    #[cfg(target_arch = "aarch64")]
    {
        const BASE:   usize = 0x0a00_0000;
        const STRIDE: usize = 0x200;
        (0..32_usize).map(|i| (BASE + i * STRIDE, 16 + i as u32))
    }
    #[cfg(target_arch = "riscv64")]
    {
        const BASE:   usize = 0x1000_1000;
        const STRIDE: usize = 0x1000;
        (0..8_usize).map(|i| (BASE + i * STRIDE, 1 + i as u32))
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "riscv64")))]
    {
        core::iter::empty()
    }
}

// ─── Init ─────────────────────────────────────────────────────────────────────

/// Probe all VirtIO MMIO slots and initialise the first Input device found.
///
/// Returns `None` if no VirtIO input device is present.
pub fn find_and_init_input() -> Option<InputDevice> {
    for (base, irq) in virtio_slot_iter() {
        // SAFETY: base is within the arch MMIO window mapped USER-accessible by paging init.
        let magic = unsafe { core::ptr::read_volatile(base as *const u32) };
        if magic != VIRTIO_MAGIC { continue; }

        let device_id = unsafe { core::ptr::read_volatile((base + 8) as *const u32) };
        if device_id != VIRTIO_DEV_INPUT { continue; }

        // Claim exclusive MMIO ownership.  This also gates the kernel poll path:
        // `virtio_input::poll_events` checks `lookup_mmio_owner(base)` and skips
        // if any cell owns it — preventing double-drain of the virtqueue.
        if sys_request_mmio(base, MMIO_SLOT_SIZE) != 0 {
            // Already claimed or not in allowlist — skip.
            continue;
        }

        // SAFETY: base validated (magic) and claimed above.
        let header = unsafe { NonNull::new_unchecked(base as *mut VirtIOHeader) };
        let transport = match unsafe { MmioTransport::new(header) } {
            Ok(t) if t.device_type() == DeviceType::Input => t,
            Ok(t) => { core::mem::forget(t); continue; }
            Err(_) => continue,
        };

        match VirtIOInput::<CellHal, MmioTransport>::new(transport) {
            Ok(dev) => return Some(InputDevice { dev, irq, base }),
            Err(_) => continue,
        }
    }
    None
}
