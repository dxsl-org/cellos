//! The system text clipboard, held by the compositor
//! (`wire::display::clipboard`).
//!
//! Whatever path receives compositor frames routes `CLIPBOARD_CHANGED` here
//! ([`note_frame`]); an app takes the newest offer with [`take_offer`] and
//! reads its text with [`ViSurface::clipboard_text`]. Setting the clipboard
//! copies the text into one Grant this task registers at the first set and
//! shares read-only with the compositor.

extern crate alloc;

use alloc::string::String;
use alloc::vec;
use core::sync::atomic::{AtomicUsize, Ordering};

use types::{ViError, ViResult};
use wire::display::{ClipboardChanged, ClipboardGet, ClipboardSet, CLIPBOARD_MAX};

use crate::syscall::{
    sys_grant_copy_rows_owned_by, sys_grant_register, sys_grant_share, sys_grant_unregister,
    GrantRows,
};

use super::ipc;
use super::surface::ViSurface;

static OFFER: crate::sync::Mutex<Option<ClipboardChanged>> = crate::sync::Mutex::new(None);
/// This task's Grant for outgoing text, 0 until the first set.
static GRANT: AtomicUsize = AtomicUsize::new(0);

/// Record a `CLIPBOARD_CHANGED` frame; a newer one replaces an untaken one.
/// Returns whether `frame` was one (well-formed).
pub fn note_frame(frame: &[u8]) -> bool {
    let Ok(event) = ClipboardChanged::decode(frame) else {
        return false;
    };
    *OFFER.lock() = Some(event);
    true
}

/// The newest clipboard offer not yet taken.
pub fn take_offer() -> Option<ClipboardChanged> {
    OFFER.lock().take()
}

/// One row of `len` bytes from the start of a Grant.
const fn row(len: usize) -> GrantRows {
    GrantRows {
        src_offset: 0,
        src_stride: len,
        dst_offset: 0,
        dst_stride: len,
        row_len: len,
        rows: 1,
    }
}

/// The Grant shared with `comp_tid` for outgoing text.
fn outgoing_grant(comp_tid: usize) -> ViResult<usize> {
    let grant = GRANT.load(Ordering::Acquire);
    if grant != 0 {
        return Ok(grant);
    }
    let grant = sys_grant_register(CLIPBOARD_MAX).ok_or(ViError::OutOfMemory)?;
    if !sys_grant_share(grant, comp_tid, 0) {
        let _ = sys_grant_unregister(grant);
        return Err(ViError::IO);
    }
    GRANT.store(grant, Ordering::Release);
    Ok(grant)
}

impl ViSurface {
    /// Put `text` on the clipboard. The compositor takes it only while this
    /// surface has keyboard focus.
    ///
    /// # Errors
    /// `InvalidInput` for empty text or text over `CLIPBOARD_MAX` bytes;
    /// `OutOfMemory` or `IO` when the Grant or the request fails.
    pub fn set_clipboard(&self, text: &str) -> ViResult<()> {
        if text.is_empty() || text.len() > CLIPBOARD_MAX {
            return Err(ViError::InvalidInput);
        }
        let grant = outgoing_grant(self.comp_tid)?;
        crate::grant::with_shared_bytes_mut(grant, |bytes| {
            bytes[..text.len()].copy_from_slice(text.as_bytes())
        })
        .ok_or(ViError::IO)?;
        let frame = ClipboardSet {
            cap: self.cap,
            grant: grant as u32,
            len: text.len() as u32,
        }
        .encode();
        ipc::send_lifecycle_request(self.comp_tid, &frame)
    }

    /// Ask for the clipboard; it arrives as an offer ([`take_offer`]) when
    /// this surface has keyboard focus, otherwise nothing arrives.
    ///
    /// # Errors
    /// `IO` when the request cannot be sent.
    pub fn request_clipboard(&self) -> ViResult<()> {
        ipc::send_lifecycle_request(self.comp_tid, &ClipboardGet { cap: self.cap }.encode())
    }

    /// The text of `offer` if it is for this surface: empty for an empty
    /// clipboard, `None` when the compositor's Grant cannot be read as its or
    /// the bytes are not UTF-8.
    pub fn clipboard_text(&self, offer: &ClipboardChanged) -> Option<String> {
        if offer.cap != self.cap {
            return None;
        }
        let len = offer.len as usize;
        if len == 0 {
            return Some(String::new());
        }
        let mut bytes = vec![0; len];
        sys_grant_copy_rows_owned_by(offer.grant as usize, self.comp_tid, row(len), &mut bytes)?;
        String::from_utf8(bytes).ok()
    }
}
