//! Clipboard text between the desktop and the guest.
//!
//! Desktop -> guest: the compositor offers its clipboard when this window
//! gains keyboard focus (`ostd::display::clipboard`); the text goes into a
//! Grant this launcher registers once and shares read-only with its
//! hypervisor, named by a `VmmClipboardRequest`. Guest -> desktop: a
//! `VmmEvent::Clipboard` names the hypervisor's Grant; the text is copied out
//! (owner-checked) and put on the desktop clipboard.
//!
//! The last text exchanged either way is remembered, so a text that came from
//! the guest and comes back as the desktop's offer is not sent to it again.

use alloc::string::String;
use alloc::vec;

use ostd::display::ViSurface;
use ostd::syscall::{
    sys_grant_copy_rows_owned_by, sys_grant_register, sys_grant_share, sys_grant_unregister,
    GrantRows,
};
use wire::vmm_session::VMM_CLIPBOARD_MAX;

use crate::guest::Guest;

const GRANT_BYTES: usize = VMM_CLIPBOARD_MAX as usize;

pub struct GuestClipboard {
    /// The Grant shared with the hypervisor, 0 until the first text.
    grant: usize,
    last: String,
    /// Length of the text in the Grant whose request the hypervisor's
    /// mailbox has not taken yet.
    pending: Option<u32>,
}

impl GuestClipboard {
    pub const fn new() -> Self {
        Self {
            grant: 0,
            last: String::new(),
            pending: None,
        }
    }

    /// Take the compositor's newest offer and hand a new text to the guest.
    /// Returns `true` while a request waits for the hypervisor's mailbox.
    pub fn poll(&mut self, surface: &ViSurface, guest: &Guest) -> bool {
        if let Some(offer) = ostd::display::clipboard::take_offer() {
            if let Some(text) = surface.clipboard_text(&offer) {
                if !text.is_empty() && text.len() <= GRANT_BYTES && text != self.last {
                    self.stage(guest, text);
                }
            }
        }
        self.flush(guest)
    }

    /// The guest copied `len` bytes at the start of `grant` (the
    /// hypervisor's): put them on the desktop clipboard.
    pub fn guest_copied(&mut self, surface: &ViSurface, guest: &Guest, grant: u32, len: u32) {
        let len = len as usize;
        let mut bytes = vec![0u8; len];
        let row = GrantRows {
            src_offset: 0,
            src_stride: len,
            dst_offset: 0,
            dst_stride: len,
            row_len: len,
            rows: 1,
        };
        if sys_grant_copy_rows_owned_by(grant as usize, guest.hypervisor_tid(), row, &mut bytes)
            .is_none()
        {
            return;
        }
        let Ok(text) = String::from_utf8(bytes) else {
            return;
        };
        if surface.set_clipboard(&text).is_ok() {
            ostd::io::println(&alloc::format!(
                "[chrome] clipboard from guest: {} bytes",
                text.len()
            ));
            self.last = text;
        }
    }

    /// Write `text` into the shared Grant and queue its request.
    fn stage(&mut self, guest: &Guest, text: String) {
        let Some(grant) = self.grant(guest.hypervisor_tid()) else {
            return;
        };
        let written = ostd::grant::with_shared_bytes_mut(grant, |dst| {
            dst.get_mut(..text.len())?.copy_from_slice(text.as_bytes());
            Some(())
        });
        if written.flatten().is_none() {
            return;
        }
        self.pending = Some(text.len() as u32);
        self.last = text;
    }

    /// Post the queued request; `true` while it still waits.
    fn flush(&mut self, guest: &Guest) -> bool {
        let Some(len) = self.pending else {
            return false;
        };
        if !guest.send_clipboard(self.grant as u32, len) {
            return true;
        }
        ostd::io::println(&alloc::format!("[chrome] clipboard to guest: {len} bytes"));
        self.pending = None;
        false
    }

    /// The Grant shared with the hypervisor, registered at the first text.
    fn grant(&mut self, hypervisor_tid: usize) -> Option<usize> {
        if self.grant == 0 {
            let grant = sys_grant_register(GRANT_BYTES)?;
            if !sys_grant_share(grant, hypervisor_tid, 0) {
                let _ = sys_grant_unregister(grant);
                return None;
            }
            self.grant = grant;
        }
        Some(self.grant)
    }
}
