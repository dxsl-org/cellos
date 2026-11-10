//! VirtIO GPU MMIO device — init + framebuffer flush for the GPU Driver Cell.
//!
//! # Unsafe island
//! This module is the ONLY place with `unsafe` code in the cell:
//!   1. `MmioTransport::new` takes a raw header pointer to the claimed slot,
//!      which the kernel identified (`sys_probe_virtio_mmio`) before the claim.
//!   2. `virtio_drivers::Hal` requires `unsafe impl`.
//!   3. Direct framebuffer pointer from `setup_framebuffer()`.
//!   4. The bytes of a [`StagedSource`]: a Grant this cell owns.

#![allow(unsafe_code)]

extern crate alloc;

use core::ptr::NonNull;
use ostd::mmio::request_region;
use ostd::syscall::{
    sys_grant_alloc, sys_grant_free, sys_grant_slice_with_len, sys_probe_virtio_mmio,
};
use virtio_drivers::{
    device::gpu::VirtIOGpu,
    transport::mmio::{MmioTransport, VirtIOHeader},
    transport::{DeviceType, Transport},
    BufferDirection, Hal, PhysAddr,
};

// ─── Constants ────────────────────────────────────────────────────────────────

/// VirtIO device type: GPU (16).
const VIRTIO_DEV_GPU: u16 = 16;

/// Window claimed per slot. RISC-V spaces slots a page apart and the kernel
/// registers each as its page, so the claim maps it for U-mode; AArch64 packs
/// eight 0x200 slots per page, whose accesses the kernel mediates.
#[cfg(target_arch = "riscv64")]
pub const MMIO_SLOT_SIZE: usize = 0x1000;
#[cfg(not(target_arch = "riscv64"))]
pub const MMIO_SLOT_SIZE: usize = 0x200;

/// Source bytes the kernel staged for one flush or cursor request.
///
/// The compositor's own buffers are never read here: the cell handles a
/// request after the compositor's syscall has returned, when those buffers may
/// already be rewritten or freed. The kernel instead copies the source into a
/// fresh Grant owned by this cell before posting the request, and names it by
/// id (`id == base` in SAS). This value takes ownership of that Grant and frees
/// it on drop, so every dispatch path releases it exactly once.
pub(crate) struct StagedSource {
    grant: usize,
    /// Mapping and registered length, or `None` if `grant` is not a Grant this
    /// cell can resolve (then there is nothing to read or free).
    mapping: Option<(*const u8, usize)>,
}

impl StagedSource {
    /// Take ownership of the staged Grant `grant` named by a kernel request.
    ///
    /// Requests are accepted only from the kernel (`sender_tid == 0`), which
    /// names only Grants it allocated for this cell, so a resolvable id here is
    /// this cell's own Grant: no other task can free or rewrite it.
    pub(crate) fn adopt(grant: usize) -> Self {
        let mapping = sys_grant_slice_with_len(grant).map(|(ptr, len)| (ptr.cast_const(), len));
        Self { grant, mapping }
    }

    /// The first `len` staged bytes, or `None` for an empty request or a Grant
    /// that does not resolve or is shorter than `len`.
    pub(crate) fn bytes(&self, len: usize) -> Option<&[u8]> {
        let (ptr, size) = self.mapping?;
        if len == 0 || len > size {
            return None;
        }
        // SAFETY: `ptr` maps this cell's own Grant of `size >= len` registered
        // bytes (see `adopt`). Only `Drop` frees it, after every borrow of
        // `self` has ended, and nothing else writes it once the kernel posted.
        Some(unsafe { core::slice::from_raw_parts(ptr, len) })
    }
}

impl Drop for StagedSource {
    fn drop(&mut self) {
        if self.mapping.is_some() {
            sys_grant_free(self.grant);
        }
    }
}

// ─── CellHal ─────────────────────────────────────────────────────────────────

/// VirtIO DMA HAL backed by `sys_grant_alloc` / `sys_grant_free`.
///
/// In Cellos SAS, physical address == virtual address, so `phys == virt == grant_id`.
pub(crate) struct CellHal;

// SAFETY: CellHal is zero-sized and stateless; all ops route through kernel syscalls.
unsafe impl Hal for CellHal {
    fn dma_alloc(pages: usize, _dir: BufferDirection) -> (PhysAddr, NonNull<u8>) {
        let base = sys_grant_alloc(pages * 4096).expect("[virtio-gpu] DMA OOM");
        let ptr = NonNull::new(base as *mut u8).expect("[virtio-gpu] DMA grant at address 0");
        (base, ptr)
    }

    unsafe fn dma_dealloc(paddr: PhysAddr, _vaddr: NonNull<u8>, _pages: usize) -> i32 {
        sys_grant_free(paddr);
        0
    }

    unsafe fn mmio_phys_to_virt(paddr: PhysAddr, _size: usize) -> NonNull<u8> {
        NonNull::new(paddr as *mut u8).expect("[virtio-gpu] MMIO at address 0")
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

    /// Copy a pixel rect from the kernel-staged `src` into the VirtIO
    /// framebuffer and flush the dirty rect to the display.
    ///
    /// `xy` packs `(x << 16) | y`; `wh` packs `(w << 16) | h`.
    /// `data_len` is the staged byte count; if it's smaller than `w*h*4`, or
    /// `src` does not hold that many bytes, the call is ignored.
    pub(crate) fn flush_rect(&mut self, src: &StagedSource, data_len: usize, xy: u32, wh: u32) {
        let x = (xy >> 16) & 0xFFFF;
        let y = xy & 0xFFFF;
        let w = (wh >> 16) & 0xFFFF;
        let h = wh & 0xFFFF;
        if w == 0 || h == 0 {
            return;
        }
        let src_stride = w as usize * 4;
        let expected = src_stride * h as usize;
        if data_len < expected {
            return;
        }
        let Some(src) = src.bytes(expected) else {
            return;
        };
        let x = x.min(self.width);
        let y = y.min(self.height);
        let w = w.min(self.width.saturating_sub(x));
        let h = h.min(self.height.saturating_sub(y));
        if w == 0 || h == 0 {
            return;
        }
        let stride = self.width as usize * 4;
        let fb = self.framebuffer();
        for row in 0..h as usize {
            let fb_off = (y as usize + row) * stride + x as usize * 4;
            let src_off = row * src_stride;
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
        match sys_probe_virtio_mmio(base) {
            Ok(identity) if identity.device_id == VIRTIO_DEV_GPU => {}
            _ => continue,
        }

        // Claim exclusive MMIO ownership.  Prevents kernel from double-initialising the slot.
        if request_region(base, MMIO_SLOT_SIZE).is_err() {
            continue;
        }

        let header =
            NonNull::new(base as *mut VirtIOHeader).expect("[virtio-gpu] VirtIO slot at address 0");
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
