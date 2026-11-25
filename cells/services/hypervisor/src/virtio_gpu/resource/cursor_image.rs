//! A guest cursor resource's pixels, for a launcher's window (client mode,
//! where the guest cursor is not drawn into the scanout).

extern crate alloc;

use alloc::vec::Vec;

use super::{copy_from_backing, pixel_len, ResourceError, ResourceTable};

/// Largest cursor edge (the virtio-gpu cursor plane is 64x64).
pub const MAX_CURSOR_EDGE: u32 = 64;

impl ResourceTable {
    /// `(width, height, pixels)` of cursor resource `resource_id`: BGRA,
    /// rows packed, the fourth byte alpha (non-premultiplied) whatever the
    /// resource's declared format: Linux guests write their cursor with alpha
    /// into an X8 resource too, and forcing it opaque drew a black square
    /// (Chrome lane, phase 08). The console path blends the same way.
    pub fn cursor_image(
        &self,
        vm_id: usize,
        resource_id: u32,
    ) -> Result<(u32, u32, Vec<u8>), ResourceError> {
        let resource = self
            .resources
            .get(&resource_id)
            .ok_or(ResourceError::InvalidResourceId)?;
        let (width, height) = (resource.width, resource.height);
        if !(1..=MAX_CURSOR_EDGE).contains(&width) || !(1..=MAX_CURSOR_EDGE).contains(&height) {
            return Err(ResourceError::InvalidParameter);
        }
        let len = pixel_len(width, height).ok_or(ResourceError::InvalidParameter)?;
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(len)
            .map_err(|_| ResourceError::OutOfMemory)?;
        pixels.resize(len, 0);
        copy_from_backing(vm_id, &resource.backing, 0, &mut pixels)?;
        Ok((width, height, pixels))
    }
}
