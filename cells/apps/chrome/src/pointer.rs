//! The guest's cursor on the window: the hypervisor's image, handed on to the
//! compositor, which shows it under the host pointer while the pointer is over
//! the window's content.
//!
//! The compositor reads only Grants the requester owns, so the image is copied
//! out of the hypervisor's cursor Grant (owner-checked) into a Grant this
//! launcher registers once and shares with the compositor; the compositor
//! copies it in turn when it receives the request.

use ostd::display::ViSurface;
use ostd::syscall::{
    sys_grant_copy_rows_owned_by, sys_grant_register, sys_grant_share, sys_grant_unregister,
    GrantRows,
};
use wire::display::{PointerImage, MAX_POINTER_EDGE};

const GRANT_BYTES: usize = MAX_POINTER_EDGE as usize * MAX_POINTER_EDGE as usize * 4;

pub struct WindowPointer {
    /// The Grant shared with the compositor, 0 until the first image.
    grant: usize,
}

impl WindowPointer {
    pub const fn new() -> Self {
        Self { grant: 0 }
    }

    /// Hide the pointer over the window (the guest hid its cursor).
    pub fn hide(&self, surface: &ViSurface) {
        let _ = surface.set_pointer_image(PointerImage::Hidden);
    }

    /// Show the guest's `width`x`height` image from `guest_grant` (owned by
    /// `hypervisor_tid`), hot spot `(hot_x, hot_y)`. An image that cannot be
    /// copied changes nothing.
    pub fn show(
        &mut self,
        surface: &ViSurface,
        comp_tid: usize,
        hypervisor_tid: usize,
        guest_grant: u32,
        (width, height): (u16, u16),
        (hot_x, hot_y): (u16, u16),
    ) {
        let Some(grant) = self.grant(comp_tid) else {
            return;
        };
        let row_len = usize::from(width) * 4;
        let rows = GrantRows {
            src_offset: 0,
            src_stride: row_len,
            dst_offset: 0,
            dst_stride: row_len,
            row_len,
            rows: usize::from(height),
        };
        let copied = ostd::grant::with_shared_bytes_mut(grant, |dst| {
            sys_grant_copy_rows_owned_by(guest_grant as usize, hypervisor_tid, rows, dst)
        });
        if copied.flatten().is_none() {
            return;
        }
        let _ = surface.set_pointer_image(PointerImage::Image {
            grant: grant as u32,
            width,
            height,
            hot_x,
            hot_y,
        });
    }

    /// The Grant shared with the compositor, registered at the first image.
    fn grant(&mut self, comp_tid: usize) -> Option<usize> {
        if self.grant == 0 {
            let grant = sys_grant_register(GRANT_BYTES)?;
            if !sys_grant_share(grant, comp_tid, 0) {
                let _ = sys_grant_unregister(grant);
                return None;
            }
            self.grant = grant;
        }
        Some(self.grant)
    }
}
