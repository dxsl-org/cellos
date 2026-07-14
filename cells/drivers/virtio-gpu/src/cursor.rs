//! Hardware cursor for the GPU Driver Cell.
//!
//! These functions are called from `main::dispatch` when the kernel forwards a
//! `GpuCursor` syscall to this cell.  Both operate on the owned `GpuDevice`.

// Inherits the `#![allow(unsafe_code)]` exemption from display.rs for the
// slice-from-raw-parts pointer (compositor cursor sprite buffer in SAS).
#![allow(unsafe_code)]

use crate::display::GpuDevice;
use ostd::io::println;

/// Byte length required by `VirtIOGpu::setup_cursor` (64×64 BGRA8888).
const SPRITE_LEN: usize = 64 * 64 * 4;

/// Upload a 64×64 BGRA8888 cursor sprite and set the initial position.
///
/// `xy` packs `(x << 16) | y` (initial screen position).
/// `hot` packs `(hot_x << 16) | hot_y` (hotspot within sprite).
/// `data_ptr` points to the sprite bytes in the compositor's SAS address space.
pub fn set_sprite(dev: &mut GpuDevice, data_ptr: usize, xy: u32, hot: u32) {
    let x = ((xy >> 16) & 0xFFFF) as u32;
    let y = (xy & 0xFFFF) as u32;
    let hot_x = ((hot >> 16) & 0xFFFF) as u32;
    let hot_y = (hot & 0xFFFF) as u32;
    // SAFETY: data_ptr is a compositor user-space pointer valid in SAS;
    // SPRITE_LEN is constant and bounds-checked by the VirtIO cursor spec.
    let image = unsafe { core::slice::from_raw_parts(data_ptr as *const u8, SPRITE_LEN) };
    if dev.gpu.setup_cursor(image, x, y, hot_x, hot_y).is_err() {
        println("[virtio-gpu] cursor setup_cursor failed");
    }
}

/// Reposition the hardware cursor without re-uploading the sprite.
///
/// `xy` packs `(x << 16) | y`.  Cheap: issues MOVE_CURSOR only (no DMA).
pub fn move_to(dev: &mut GpuDevice, xy: u32) {
    let x = ((xy >> 16) & 0xFFFF) as u32;
    let y = (xy & 0xFFFF) as u32;
    let _ = dev.gpu.move_cursor(x, y);
}
