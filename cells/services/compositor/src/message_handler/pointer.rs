//! `SET_POINTER_IMAGE`: a surface owner chooses what the pointer looks like
//! over the surface's content.

extern crate alloc;

use alloc::rc::Rc;
use ostd::syscall::{sys_grant_copy_rows_owned_by, GrantRows};
use wire::display::{PointerImage, SetPointerImage};

use crate::pointer_look::{PointerLook, PointerSprite};
use crate::surface_table::SurfaceTable;

/// Owner-only. An image is copied out of the sender's own Grant here (the
/// kernel refuses a Grant the sender does not own, and one shorter than the
/// image); a request that fails any check changes nothing. The look shows at
/// the compositor's next turn (`InputState::refresh_look`).
pub(super) fn set_pointer_image(buf: &[u8; 512], sender: usize, table: &mut SurfaceTable) {
    let Ok(request) = SetPointerImage::decode(buf) else {
        return;
    };
    let Some(surface) = table
        .get_mut(u64::from(request.cap))
        .filter(|surface| surface.owner == sender)
    else {
        return;
    };
    surface.pointer = match request.image {
        PointerImage::Arrow => PointerLook::Arrow,
        PointerImage::Hidden => PointerLook::Hidden,
        PointerImage::Image {
            grant,
            width,
            height,
            hot_x,
            hot_y,
        } => {
            let row_len = usize::from(width) * 4;
            let mut pixels = alloc::vec![0u8; row_len * usize::from(height)];
            let rows = GrantRows {
                src_offset: 0,
                src_stride: row_len,
                dst_offset: 0,
                dst_stride: row_len,
                row_len,
                rows: usize::from(height),
            };
            if sys_grant_copy_rows_owned_by(grant as usize, sender, rows, &mut pixels).is_none() {
                return;
            }
            let Some(sprite) = PointerSprite::new(
                u32::from(width),
                u32::from(height),
                u32::from(hot_x),
                u32::from(hot_y),
                pixels,
            ) else {
                return;
            };
            PointerLook::Image(Rc::new(sprite))
        }
    };
}
