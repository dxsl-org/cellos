//! VirtIO-MMIO transport path (riscv64 / aarch64 QEMU virt boards).
//!
//! # Unsafe island
//! `unsafe` here serves two purposes:
//!   1. `virtio_drivers::Hal` requires `unsafe impl` (trait is `unsafe trait`).
//!   2. `MmioTransport::new` takes a raw header pointer to the claimed slot.
//!
//! Slots are identified by the kernel (`sys_probe_virtio_mmio`): the VirtIO
//! window is kernel-only until a claim grants it.

#![allow(unsafe_code)]

use crate::device::{CellNet, NetDevice, NET_QUEUE_SIZE, RX_BUF_LEN};
use core::ptr::NonNull;
use ostd::syscall::{sys_grant_alloc, sys_grant_free, sys_probe_virtio_mmio, sys_wait_irq};
use virtio_drivers::{
    device::net::VirtIONet,
    transport::mmio::{MmioTransport, VirtIOHeader},
    transport::{DeviceType, Transport},
    BufferDirection, Hal, PhysAddr,
};

// ─── Constants ───────────────────────────────────────────────────────────────

/// VirtIO device type: Network (1).
const VIRTIO_DEV_NET: u16 = 1;

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
/// In Cellos SAS, physical address == virtual address (identity mapping), so
/// `phys == virt == grant_id` for all grant-allocated pages.
pub(crate) struct CellHal;

// SAFETY: CellHal is a zero-sized stateless type; all ops go through kernel syscalls.
unsafe impl Hal for CellHal {
    fn dma_alloc(pages: usize, _dir: BufferDirection) -> (PhysAddr, NonNull<u8>) {
        let base = sys_grant_alloc(pages * 4096).expect("[virtio-net] DMA OOM");
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
        // Cell image memory (heap/.bss/stack of a loaded cell) lives at loader
        // VAs (e.g. 0x1_0800_0000) that are NOT identity-mapped — the device
        // cannot DMA there (QEMU maps the bogus address as zero-length → device
        // marked broken). Bounce through an identity-mapped grant page instead.
        // Grant pages satisfy vaddr == paddr, so the returned base is both.
        let len = buffer.len();
        let bounce = sys_grant_alloc(len).expect("[virtio-net] bounce OOM");
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
        // Copy device-written bytes back into the driver's buffer, then release
        // the bounce page. paddr == grant base (see share()).
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

impl NetDevice {
    /// Block until an RX IRQ fires, then try to receive a frame.
    ///
    /// Returns byte count written to `buf` (0 = still nothing after IRQ wake).
    // reason: not called by the current polling main loop — see irq/mmio_base
    // on `NetDevice`.
    #[allow(dead_code)]
    pub fn wait_recv(&mut self, buf: &mut [u8]) -> usize {
        // Lost-wakeup guard: sys_wait_irq checks take_pending before parking,
        // so an IRQ that fired after try_recv but before this call is not lost.
        let _ = sys_wait_irq(self.irq, self.mmio_base);
        self.try_recv(buf)
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

/// Discovery rounds (one yield apart) while a network slot is still held,
/// before concluding none is free: a restarted cell can probe before its
/// predecessor's claim is reaped. The bound the PCI driver cells use.
const FIND_ATTEMPTS: usize = 200;

/// Probe all VirtIO MMIO slots and initialise the first Network device found.
///
/// A network slot another Cell still holds is retried, a yield apart, for at
/// most [`FIND_ATTEMPTS`] rounds; with no network slot at all the scan ends
/// after one round. Returns `None` if no VirtIO NIC is claimable (graceful
/// exit on non-VirtIO platforms).
pub fn find_and_init_net() -> Option<NetDevice> {
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

/// One pass over the slots; sets `held` when a network slot's claim failed.
fn init_first_free(held: &mut bool) -> Option<NetDevice> {
    for (base, irq) in virtio_slot_iter() {
        // VirtIO device of another type — never claim it (the claim is
        // exclusive) nor create an MmioTransport on it (that resets it).
        match sys_probe_virtio_mmio(base) {
            Ok(identity) if identity.device_id == VIRTIO_DEV_NET => {}
            _ => continue,
        }

        // Claim exclusive MMIO ownership via the kernel resource registry.
        if ostd::mmio::request_region(base, MMIO_SLOT_SIZE).is_err() {
            // Owned by another Cell, live or awaiting reap.
            *held = true;
            continue;
        }

        // Create VirtIO transport.
        // SAFETY: the kernel identified a VirtIO network transport at `base` and
        // granted this Cell the slot above, so the header is live device memory
        // this Cell may access.
        let header = unsafe { NonNull::new_unchecked(base as *mut VirtIOHeader) };
        let transport = match unsafe { MmioTransport::new(header) } {
            Ok(t) if t.device_type() == DeviceType::Network => t,
            Ok(t) => {
                // Magic matched but type changed between our probe and transport init (race).
                // Forget to avoid resetting the device; drop our MMIO claim and move on.
                core::mem::forget(t);
                continue;
            }
            Err(_) => continue,
        };

        // Initialise virtqueues and allocate RX buffers.
        let net: CellNet =
            match VirtIONet::<CellHal, MmioTransport, NET_QUEUE_SIZE>::new(transport, RX_BUF_LEN) {
                Ok(n) => n,
                Err(e) => {
                    let _ = e;
                    continue;
                }
            };

        return Some(NetDevice {
            net,
            irq: irq as u8,
            mmio_base: base,
        });
    }
    None
}
