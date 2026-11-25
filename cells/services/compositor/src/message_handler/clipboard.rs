//! `CLIPBOARD_SET` / `CLIPBOARD_GET`: the rules live in [`crate::clipboard`].

extern crate alloc;

use wire::display::{compositor_ops, ClipboardGet, ClipboardSet};

use crate::clipboard::{Clipboard, Focus, Syscalls};
use crate::surface_table::SurfaceTable;

/// A malformed or refused request changes nothing and gets no reply.
pub(super) fn handle(
    buf: &[u8; 512],
    sender: usize,
    focus: Focus,
    table: &mut SurfaceTable,
    clipboard: &mut Clipboard,
) {
    match buf[0] {
        compositor_ops::CLIPBOARD_SET => {
            let Ok(request) = ClipboardSet::decode(buf) else {
                return;
            };
            if let Some(len) = clipboard.set(&request, sender, focus, table, &mut Syscalls) {
                // The length only: the text is the user's.
                ostd::io::println(&alloc::format!("[compositor] clipboard set: {len} bytes"));
            }
        }
        compositor_ops::CLIPBOARD_GET => {
            if let Ok(request) = ClipboardGet::decode(buf) {
                clipboard.get(&request, sender, focus, table, &mut Syscalls);
            }
        }
        _ => {}
    }
}
