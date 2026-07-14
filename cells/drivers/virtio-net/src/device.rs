//! VirtIO-MMIO NIC device layer for the virtio-net Driver Cell.
//!
//! # Unsafe island
//! This module is the ONLY place in the cell with `unsafe` code, for two reasons:
//!   1. `virtio_drivers::Hal` requires `unsafe impl` (trait is `unsafe trait`).
//!   2. MMIO probe reads (`read_volatile`) must happen before the kernel resource
//!      registry accepts the claim — we read magic/device_id, THEN claim.
//!
//! All other modules in this cell are `#![forbid(unsafe_code)]`.

#![allow(unsafe_code)]

extern crate alloc;

use core::ptr::NonNull;
use ostd::syscall::{sys_grant_alloc, sys_grant_free, sys_wait_irq};
use virtio_drivers::{
    device::net::VirtIONet,
    transport::mmio::{MmioTransport, VirtIOHeader},
    transport::{DeviceType, Transport},
    BufferDirection, Hal, PhysAddr,
};

// ─── Constants ───────────────────────────────────────────────────────────────

/// VirtIO MMIO magic value ("virt" in little-endian ASCII).
const VIRTIO_MAGIC: u32 = 0x7472_6976;

/// VirtIO device type: Network (1).
const VIRTIO_DEV_NET: u32 = 1;

/// VirtIO MMIO register area size per slot — 0x200 on both QEMU virt boards.
pub const MMIO_SLOT_SIZE: usize = 0x200;

/// RX/TX virtqueue depth. Power-of-two; QEMU supports up to 1024.
const NET_QUEUE_SIZE: usize = 16;

/// RX buffer length per slot. Must be ≥ 1526 (MAX Ethernet + VirtioNetHdr).
const RX_BUF_LEN: usize = 2048;

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

// ─── Device state ─────────────────────────────────────────────────────────────

pub(crate) type CellNet = VirtIONet<CellHal, MmioTransport, NET_QUEUE_SIZE>;

/// Runtime state for the active VirtIO NIC.
pub struct NetDevice {
    pub(crate) net: CellNet,
    // reason: consumed by wait_recv()'s IRQ-wake path below; the cell's current
    // main loop uses try_recv() polling instead, so both this field pair and
    // wait_recv() are dormant until the IRQ-driven path is wired in.
    #[allow(dead_code)]
    pub irq: u8,
    #[allow(dead_code)]
    pub mmio_base: usize,
}

impl NetDevice {
    /// Get the MAC address of this NIC.
    pub fn mac(&self) -> [u8; 6] {
        self.net.mac_address()
    }

    /// Transmit `frame`. Returns `true` on success.
    pub fn send(&mut self, frame: &[u8]) -> bool {
        let mut tx = self.net.new_tx_buffer(frame.len());
        tx.packet_mut().copy_from_slice(frame);
        self.net.send(tx).is_ok()
    }

    /// Try to receive one frame into `buf`. Returns byte count (0 = nothing ready).
    pub fn try_recv(&mut self, buf: &mut [u8]) -> usize {
        match self.net.receive() {
            Ok(rx) => {
                let len = rx.packet_len().min(buf.len());
                buf[..len].copy_from_slice(&rx.packet()[..len]);
                if let Err(e) = self.net.recycle_rx_buffer(rx) {
                    // Non-fatal: log and continue; packet is already in buf.
                    let _ = e;
                }
                len
            }
            Err(_) => 0,
        }
    }

    /// Block until an RX IRQ fires, then try to receive a frame.
    ///
    /// Returns byte count written to `buf` (0 = still nothing after IRQ wake).
    // reason: not called by the current polling main loop — see irq/mmio_base above.
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

/// Probe all VirtIO MMIO slots and initialise the first Network device found.
///
/// Returns `None` if no VirtIO NIC is present (graceful exit on non-VirtIO platforms).
pub fn find_and_init_net() -> Option<NetDevice> {
    for (base, irq) in virtio_slot_iter() {
        // Safety probe: read VirtIO magic directly (U-mode, identity-mapped after paging fix).
        // SAFETY: base is within the arch MMIO window mapped USER-accessible by init_kernel_paging.
        let magic = unsafe { core::ptr::read_volatile(base as *const u32) };
        if magic != VIRTIO_MAGIC {
            continue;
        }

        // Check VirtIO device type at offset 8.
        // SAFETY: same invariant as above; base + 8 is within the same 0x200-byte slot.
        let device_id = unsafe { core::ptr::read_volatile((base + 8) as *const u32) };
        if device_id != VIRTIO_DEV_NET {
            // VirtIO device of another type — do NOT create MmioTransport (would reset it).
            continue;
        }

        // Claim exclusive MMIO ownership via the kernel resource registry.
        // On RISC-V the slot size in PCIE_BARS is 0x200 (registered by drivers::init()).
        if ostd::mmio::request_region(base, MMIO_SLOT_SIZE).is_err() {
            // Already owned or not in allowlist — skip.
            continue;
        }

        // Create VirtIO transport.
        // SAFETY: base was validated (magic check) and claimed above; it is a live
        // VirtIO MMIO header within the USER-accessible identity-mapped window.
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
        let net =
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
