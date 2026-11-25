// SPDX-License-Identifier: MIT
//! The Terminal's side of the system clipboard (`ostd::display::clipboard`): copy puts
//! the selected text there, paste asks the compositor for it and feeds the answer to the
//! session's paste path.

extern crate alloc;
use alloc::format;
use alloc::string::String;

use ostd::display::clipboard::take_offer;
use ostd::display::ViSurface;
use ostd::syscall::sys_get_time_ms;

/// How long a paste waits for the compositor's answer. The answer comes at once when
/// this surface has focus and never otherwise; an expired request keeps a later
/// unsolicited offer (sent on focus gain) from turning into a paste nobody asked for.
const PASTE_WAIT_MS: u64 = 1000;

/// Put `text` on the clipboard; blank selections copy nothing.
pub fn copy(surf: &ViSurface, text: &str) {
    if text.is_empty() {
        return;
    }
    match surf.set_clipboard(text) {
        Ok(()) => ostd::io::println(&format!("[terminal] copied {} bytes", text.len())),
        Err(error) => ostd::io::println(&format!("[terminal] copy failed: {:?}", error)),
    }
}

/// One outstanding Ctrl+Shift+V.
#[derive(Debug, Default)]
pub struct PasteRequest {
    /// `sys_get_time_ms` after which the request is forgotten; `None` = no paste pending.
    deadline_ms: Option<u64>,
}

impl PasteRequest {
    /// Ask the compositor for the clipboard; a repeat while pending restarts the wait.
    pub fn request(&mut self, surf: &ViSurface) {
        match surf.request_clipboard() {
            Ok(()) => self.deadline_ms = Some(now_ms().saturating_add(PASTE_WAIT_MS)),
            Err(error) => {
                ostd::io::println(&format!("[terminal] paste request failed: {:?}", error))
            }
        }
    }

    /// Take this loop turn's clipboard offer. Returns its text when a paste was pending
    /// (the request is then done); an offer with no paste pending is dropped.
    pub fn take_text(&mut self, surf: &ViSurface) -> Option<String> {
        let offer = take_offer();
        let deadline = self.deadline_ms?;
        let Some(offer) = offer else {
            if now_ms() > deadline {
                self.deadline_ms = None;
                ostd::io::println("[terminal] paste request unanswered");
            }
            return None;
        };
        self.deadline_ms = None;
        let text = surf.clipboard_text(&offer);
        if text.is_none() {
            ostd::io::println("[terminal] clipboard offer unreadable");
        }
        text
    }
}

fn now_ms() -> u64 {
    // Without a clock the request never expires; the next offer answers it.
    sys_get_time_ms().unwrap_or(0)
}
