//! Hardware cursor for the GPU Driver Cell.
//!
//! These functions are called from `main::dispatch` when the kernel forwards a
//! `GpuCursor` syscall to this cell.  Both operate on the owned `GpuDevice`.

use crate::display::{GpuDevice, StagedSource};
use ostd::io::println;

/// Byte length required by `VirtIOGpu::setup_cursor` (64×64 BGRA8888).
const SPRITE_LEN: usize = 64 * 64 * 4;

/// Upload a 64×64 BGRA8888 cursor sprite and set the initial position.
///
/// `xy` packs `(x << 16) | y` (initial screen position).
/// `hot` packs `(hot_x << 16) | hot_y` (hotspot within sprite).
/// `sprite` is the kernel-staged copy of the compositor's sprite; one that
/// does not hold a full sprite is ignored.
pub fn set_sprite(dev: &mut GpuDevice, sprite: &StagedSource, xy: u32, hot: u32) {
    let x = (xy >> 16) & 0xFFFF;
    let y = xy & 0xFFFF;
    let hot_x = (hot >> 16) & 0xFFFF;
    let hot_y = hot & 0xFFFF;
    let Some(image) = sprite.bytes(SPRITE_LEN) else {
        println("[virtio-gpu] cursor sprite not staged");
        return;
    };
    if dev.gpu.setup_cursor(image, x, y, hot_x, hot_y).is_err() {
        println("[virtio-gpu] cursor setup_cursor failed");
    }
}

/// Reposition the hardware cursor without re-uploading the sprite.
///
/// `xy` packs `(x << 16) | y`.  Cheap: issues MOVE_CURSOR only (no DMA).
pub fn move_to(dev: &mut GpuDevice, xy: u32) {
    let x = (xy >> 16) & 0xFFFF;
    let y = xy & 0xFFFF;
    let _ = dev.gpu.move_cursor(x, y);
}
