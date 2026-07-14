//! VirtIO GPU MMIO device — init + framebuffer flush for the GPU Driver Cell.
//!
//! # Unsafe island
//! This module is the ONLY place with `unsafe` code in the cell:
//!   1. MMIO probe reads (`read_volatile`) before kernel claim.
//!   2. `virtio_drivers::Hal` requires `unsafe impl`.
//!   3. Direct framebuffer pointer from `setup_framebuffer()`.

#![allow(unsafe_code)]

extern crate alloc;

use core::ptr::NonNull;
use ostd::mmio::request_region;
use ostd::syscall::{sys_grant_alloc, sys_grant_free};
use virtio_drivers::{
    device::gpu::VirtIOGpu,
    transport::mmio::{MmioTransport, VirtIOHeader},
    transport::{DeviceType, Transport},
    BufferDirection, Hal, PhysAddr,
};

// ─── Constants ────────────────────────────────────────────────────────────────

/// VirtIO MMIO magic value ("virt" in little-endian ASCII).
const VIRTIO_MAGIC: u32 = 0x7472_6976;

/// VirtIO device type: GPU (16).
const VIRTIO_DEV_GPU: u32 = 16;

/// VirtIO MMIO register area size per slot.
pub const MMIO_SLOT_SIZE: usize = 0x200;

// ─── CellHal ─────────────────────────────────────────────────────────────────

/// VirtIO DMA HAL backed by `sys_grant_alloc` / `sys_grant_free`.
///
/// In Cellos SAS, physical address == virtual address, so `phys == virt == grant_id`.
pub(crate) struct CellHal;

// SAFETY: CellHal is zero-sized and stateless; all ops route through kernel syscalls.
unsafe impl Hal for CellHal {
    fn dma_alloc(pages: usize, _dir: BufferDirection) -> (PhysAddr, NonNull<u8>) {
        let base = sys_grant_alloc(pages * 4096).expect("[virtio-gpu] DMA OOM");
        // SAFETY: base is non-null page-aligned address from the kernel frame allocator.
        (base, unsafe { NonNull::new_unchecked(base as *mut u8) })
    }

    unsafe fn dma_dealloc(paddr: PhysAddr, _vaddr: NonNull<u8>, _pages: usize) -> i32 {
        sys_grant_free(paddr);
        0
    }

    unsafe fn mmio_phys_to_virt(paddr: PhysAddr, _size: usize) -> NonNull<u8> {
        // SAFETY: SAS identity mapping — phys == virt.
        unsafe { NonNull::new_unchecked(paddr as *mut u8) }
    }

    unsafe fn share(buffer: NonNull<[u8]>, dir: BufferDirection) -> PhysAddr {
        // Cell image memory (heap/.bss/stack of a loaded cell) lives at loader
        // VAs (e.g. 0x1_0800_0000) that are NOT identity-mapped — the device
        // cannot DMA there (QEMU reads the bogus address as garbage → the control
        // queue transaction never completes and resolution()/flush hangs). Bounce
        // through an identity-mapped grant page instead (vaddr == paddr for grants).
        let len = buffer.len();
        let bounce = sys_grant_alloc(len).expect("[virtio-gpu] bounce OOM");
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

type GpuDev = VirtIOGpu<CellHal, MmioTransport>;

/// Owned VirtIO GPU device and its framebuffer.
pub struct GpuDevice {
    pub(crate) gpu: GpuDev,
    fb_ptr: *mut u8,
    fb_len: usize,
    pub width: u32,
    pub height: u32,
}

// SAFETY: GpuDevice is used from a single-threaded Cell event loop.
unsafe impl Send for GpuDevice {}

impl GpuDevice {
    fn framebuffer(&mut self) -> &mut [u8] {
        // SAFETY: fb_ptr + fb_len come from VirtIOGpu::setup_framebuffer() and are stable.
        unsafe { core::slice::from_raw_parts_mut(self.fb_ptr, self.fb_len) }
    }

    /// Copy a pixel rect from `src_ptr` (SAS-accessible compositor buffer) into
    /// the VirtIO framebuffer and flush the dirty rect to the display.
    ///
    /// `xy` packs `(x << 16) | y`; `wh` packs `(w << 16) | h`.
    /// `data_len` is the compositor's byte count; if it's smaller than `w*h*4`
    /// the call is silently ignored to prevent out-of-bounds reads.
    pub fn flush_rect(&mut self, src_ptr: usize, data_len: usize, xy: u32, wh: u32) {
        let x = ((xy >> 16) & 0xFFFF) as u32;
        let y = (xy & 0xFFFF) as u32;
        let w = ((wh >> 16) & 0xFFFF) as u32;
        let h = (wh & 0xFFFF) as u32;
        if w == 0 || h == 0 {
            return;
        }
        let expected = (w as usize) * (h as usize) * 4;
        if data_len < expected {
            return;
        }
        let x = x.min(self.width);
        let y = y.min(self.height);
        let w = w.min(self.width.saturating_sub(x));
        let h = h.min(self.height.saturating_sub(y));
        if w == 0 || h == 0 {
            return;
        }
        let stride = self.width as usize * 4;
        // SAFETY: src_ptr is a compositor user-space pointer valid in SAS (same address space);
        // data_len was validated above against w*h*4.
        let src = unsafe { core::slice::from_raw_parts(src_ptr as *const u8, data_len) };
        let fb = self.framebuffer();
        for row in 0..h as usize {
            let fb_off = (y as usize + row) * stride + x as usize * 4;
            let src_off = row * w as usize * 4;
            let row_bytes = w as usize * 4;
            if fb_off + row_bytes <= fb.len() && src_off + row_bytes <= src.len() {
                fb[fb_off..fb_off + row_bytes].copy_from_slice(&src[src_off..src_off + row_bytes]);
            }
        }
        let offset = (y as u64 * self.width as u64 + x as u64) * 4;
        if self.gpu.flush_rect(x, y, w, h, offset).is_err() {
            // Full flush fallback — display must never stay stale.
            let _ = self.gpu.flush();
        }
    }
}

// ─── Slot iterator ────────────────────────────────────────────────────────────

/// Yields `(mmio_base, irq)` for each VirtIO MMIO slot on the current platform.
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

// ─── Init ─────────────────────────────────────────────────────────────────────

/// Probe all VirtIO MMIO slots and initialise the first GPU device found.
///
/// Returns `None` if no VirtIO GPU is present.
pub fn find_and_init_gpu() -> Option<GpuDevice> {
    for (base, _irq) in virtio_slot_iter() {
        // SAFETY: base is within the arch MMIO window mapped USER-accessible by paging init.
        let magic = unsafe { core::ptr::read_volatile(base as *const u32) };
        if magic != VIRTIO_MAGIC {
            continue;
        }
        let device_id = unsafe { core::ptr::read_volatile((base + 8) as *const u32) };
        if device_id != VIRTIO_DEV_GPU {
            continue;
        }

        // Claim exclusive MMIO ownership.  Prevents kernel from double-initialising the slot.
        if request_region(base, MMIO_SLOT_SIZE).is_err() {
            continue;
        }

        // SAFETY: base was validated (magic + device type) and exclusively claimed above.
        let header = unsafe { NonNull::new_unchecked(base as *mut VirtIOHeader) };
        let transport = match unsafe { MmioTransport::new(header) } {
            Ok(t) if t.device_type() == DeviceType::GPU => t,
            Ok(t) => {
                core::mem::forget(t);
                continue;
            }
            Err(_) => continue,
        };

        let mut gpu = match VirtIOGpu::<CellHal, MmioTransport>::new(transport) {
            Ok(g) => g,
            Err(_) => continue,
        };

        let (width, height) = gpu.resolution().unwrap_or((1280, 800));
        let fb_slice = match gpu.setup_framebuffer() {
            Ok(s) => s,
            Err(_) => continue,
        };
        let fb_ptr = fb_slice.as_mut_ptr();
        let fb_len = fb_slice.len();
        let _ = gpu.flush();
        return Some(GpuDevice {
            gpu,
            fb_ptr,
            fb_len,
            width,
            height,
        });
    }
    None
}
