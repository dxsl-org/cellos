#![no_std]
#![forbid(unsafe_code)]

//! GPU driver Cell — thin wrapper exposing `sys_gpu_flush` to sibling cells.
//!
//! This cell has no IPC loop of its own; it provides library functions that
//! the compositor cell (or any graphics-capable cell) calls directly, since
//! all cells share the same address space.
//!
//! The actual GPU hardware interaction happens in the kernel-side VirtIO GPU
//! driver (`kernel/src/task/drivers/virtio_gpu.rs`) via the `GpuFlush` syscall.

use api::display::Rect;
use types::ViResult;

/// Flush a rectangular region of BGRA8888 pixels to the VirtIO GPU.
///
/// `pixels` must contain exactly `rect.w * rect.h * 4` bytes.
///
/// # Errors
/// Returns `ViError::IO` if the GPU driver is not initialised.
pub fn flush_rect(pixels: &[u8], rect: Rect) -> ViResult<()> {
    ostd::syscall::sys_gpu_flush(pixels, rect.x as u32, rect.y as u32, rect.w, rect.h)
        .map_err(|_| types::ViError::IO)
}

/// Fill a solid-colour rectangle on the GPU framebuffer.
///
/// `rgba` is a packed `0xRRGGBBAA` value; pixels are written in BGRA order.
/// Useful for clearing a region before compositing.
pub fn fill_rect(rect: Rect, rgba: u32) -> ViResult<()> {
    let b = ((rgba >> 8) & 0xFF) as u8;
    let g = ((rgba >> 16) & 0xFF) as u8;
    let r = ((rgba >> 24) & 0xFF) as u8;
    let a = (rgba & 0xFF) as u8;
    let n = (rect.w * rect.h) as usize;
    let mut pixels = alloc::vec![0u8; n * 4];
    for i in 0..n {
        let off = i * 4;
        pixels[off] = b;
        pixels[off + 1] = g;
        pixels[off + 2] = r;
        pixels[off + 3] = a;
    }
    flush_rect(&pixels, rect)
}

extern crate alloc;
