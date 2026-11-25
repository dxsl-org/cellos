//! The guest's cursor image, for the launcher that shows the guest's window.
//!
//! The image goes into a Grant this Cell registers once (64x64 BGRA) and
//! shares read-only with the launcher, and a `VmmEvent::Pointer` says what is
//! in it; a hidden cursor is `VmmEvent::PointerHidden`. The launcher copies the
//! image when it handles the event, so an image written while it copies shows
//! torn until the next one. Events are posted: one the launcher's queue cannot
//! take waits for the next poll, and only the latest is kept.

extern crate alloc;

use alloc::vec::Vec;
use ostd::syscall::{
    sys_grant_register, sys_grant_share, sys_grant_unregister, sys_post, SyscallResult,
};
use wire::vmm_session::VmmEvent;

use super::resource::MAX_CURSOR_EDGE;

const GRANT_BYTES: usize = (MAX_CURSOR_EDGE * MAX_CURSOR_EDGE * 4) as usize;

/// A cursor image: `(width, height, hot_x, hot_y, pixels)`, BGRA rows packed.
pub type CursorImage = (u32, u32, u32, u32, Vec<u8>);

pub struct ClientPointer {
    /// The cursor Grant, 0 until the first image.
    grant: usize,
    pending: Option<VmmEvent>,
}

impl ClientPointer {
    pub const fn new() -> Self {
        Self {
            grant: 0,
            pending: None,
        }
    }

    /// The guest set its cursor to `image`, or hid it (`None`).
    pub fn update(&mut self, client_tid: usize, image: Option<CursorImage>) {
        let event = match image {
            None => VmmEvent::PointerHidden,
            Some((width, height, hot_x, hot_y, pixels)) => {
                let Some(grant) = self.grant(client_tid) else {
                    return;
                };
                let written = ostd::grant::with_shared_bytes_mut(grant, |bytes| {
                    let dst = bytes.get_mut(..pixels.len())?;
                    dst.copy_from_slice(&pixels);
                    Some(())
                });
                if written.flatten().is_none() {
                    return;
                }
                // The guest's hot spot may lie outside its image; the
                // compositor takes it only inside.
                VmmEvent::Pointer {
                    grant: grant as u32,
                    width: width as u16,
                    height: height as u16,
                    hot_x: hot_x.min(width - 1) as u16,
                    hot_y: hot_y.min(height - 1) as u16,
                }
            }
        };
        self.pending = Some(event);
        self.poll(client_tid);
    }

    /// Post the latest event the launcher has not taken yet.
    pub fn poll(&mut self, client_tid: usize) {
        let Some(event) = self.pending else {
            return;
        };
        if matches!(sys_post(client_tid, &event.encode()), SyscallResult::Ok(0)) {
            self.pending = None;
        }
    }

    /// The cursor Grant, registered and shared at the first image.
    fn grant(&mut self, client_tid: usize) -> Option<usize> {
        if self.grant == 0 {
            let grant = sys_grant_register(GRANT_BYTES)?;
            if !sys_grant_share(grant, client_tid, 0) {
                let _ = sys_grant_unregister(grant);
                return None;
            }
            self.grant = grant;
        }
        Some(self.grant)
    }
}
