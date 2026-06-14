//! VirtIO GPU hardware cursor — wraps `VirtIOGpu::setup_cursor` / `move_cursor`.
//!
//! `setup_cursor` allocates a DMA page and issues a synchronous control-queue
//! request; it must be called once at compositor startup (op=0), not per frame.
//! `move_cursor` is cheap (MOVE_CURSOR only, no DMA), safe to call on each
//! MouseMove event.
//!
//! Both functions lock `GPU_CONTEXT`; the lock scope is minimal (no heap alloc
//! under the lock — the DMA alloc happens inside the virtio-drivers layer).

use super::GPU_CONTEXT;
use types::{ViError, ViResult};

/// Exact byte length required by `VirtIOGpu::setup_cursor` (64×64 BGRA8888).
const SPRITE_LEN: usize = 64 * 64 * 4;

/// Upload a 64×64 BGRA8888 cursor sprite and position the hotspot.
///
/// `image` must be exactly [`SPRITE_LEN`] bytes (returns `InvalidArgument`
/// otherwise). `(x, y)` is the initial cursor screen position; `(hot_x, hot_y)`
/// is the hotspot within the sprite (typically (0,0) for a top-left arrow).
///
/// Returns `Err(IO)` when the GPU is not initialised or the driver returns an
/// error (e.g. no cursor resource in the QEMU config).
pub fn set_sprite(image: &[u8], x: u32, y: u32, hot_x: u32, hot_y: u32) -> ViResult<()> {
    if image.len() != SPRITE_LEN {
        log::warn!(
            "[gpu] cursor sprite: expected {} bytes, got {}",
            SPRITE_LEN,
            image.len()
        );
        return Err(ViError::InvalidArgument);
    }

    let mut guard = GPU_CONTEXT.lock();
    let Some(ctx) = guard.as_mut() else {
        log::warn!("[gpu] cursor set_sprite: GPU not initialised");
        return Err(ViError::IO);
    };

    match ctx.gpu.setup_cursor(image, x, y, hot_x, hot_y) {
        Ok(()) => {
            log::info!("[gpu] cursor sprite uploaded (pos={},{} hot={},{})", x, y, hot_x, hot_y);
            Ok(())
        }
        Err(e) => {
            log::warn!("[gpu] cursor setup_cursor failed: {:?}", e);
            Err(ViError::IO)
        }
    }
}

/// Reposition the hardware cursor to `(x, y)`.
///
/// Cheap — issues MOVE_CURSOR only (no DMA, no sprite re-upload).
/// Returns `Err(IO)` when the GPU is not initialised.
pub fn move_to(x: u32, y: u32) -> ViResult<()> {
    let mut guard = GPU_CONTEXT.lock();
    let Some(ctx) = guard.as_mut() else {
        return Err(ViError::IO);
    };

    match ctx.gpu.move_cursor(x, y) {
        Ok(()) => Ok(()),
        Err(e) => {
            log::warn!("[gpu] cursor move_cursor failed: {:?}", e);
            Err(ViError::IO)
        }
    }
}
