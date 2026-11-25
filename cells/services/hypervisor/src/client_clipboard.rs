//! Clipboard text between the guest and the launcher of an app session
//! (`wire::vmm_session::clipboard`).
//!
//! Guest -> launcher: the text goes into one Grant this Cell registers at the
//! first copy and shares read-only with the launcher, and a
//! `VmmEvent::Clipboard` says how long it is. Grants belong to the task that
//! registered them, so all of this runs on the BSP's thread, which serves the
//! session. The launcher copies the text when it handles the event: a newer
//! copy written meanwhile can tear it, and the newer event follows anyway.
//! Events are posted; one the launcher's queue cannot take waits for the next
//! poll, and only the newest is kept.

extern crate alloc;

use alloc::string::String;
use alloc::vec;
use ostd::io::println;
use ostd::syscall::{
    sys_grant_copy_rows_owned_by, sys_grant_register, sys_grant_share, sys_grant_unregister,
    sys_post, GrantRows, SyscallResult,
};
use wire::vmm_session::{VmmClipboardRequest, VmmEvent, VMM_CLIPBOARD_MAX};

use crate::app_session_x86::AppSession;
use crate::virtio_console::Console;
use crate::virtio_mmio::VirtioMmio;

const GRANT_BYTES: usize = VMM_CLIPBOARD_MAX as usize;

pub struct ClientClipboard {
    /// The outgoing Grant, 0 until the guest's first copy.
    grant: usize,
    pending: Option<VmmEvent>,
}

impl ClientClipboard {
    pub const fn new() -> Self {
        Self {
            grant: 0,
            pending: None,
        }
    }

    /// The guest copied `text` (1..=`VMM_CLIPBOARD_MAX` bytes, UTF-8).
    pub fn send(&mut self, client_tid: usize, text: &str) {
        let Some(grant) = self.grant(client_tid) else {
            return;
        };
        let written = ostd::grant::with_shared_bytes_mut(grant, |bytes| {
            bytes
                .get_mut(..text.len())?
                .copy_from_slice(text.as_bytes());
            Some(())
        });
        if written.flatten().is_none() {
            return;
        }
        println(&alloc::format!(
            "[hv-x86] clipboard from guest: {} bytes",
            text.len()
        ));
        self.pending = Some(VmmEvent::Clipboard {
            grant: grant as u32,
            len: text.len() as u32,
        });
        self.poll(client_tid);
    }

    /// Post the newest event the launcher has not taken yet.
    pub fn poll(&mut self, client_tid: usize) {
        let Some(event) = self.pending else {
            return;
        };
        if matches!(sys_post(client_tid, &event.encode()), SyscallResult::Ok(0)) {
            self.pending = None;
        }
    }

    /// The outgoing Grant, registered and shared at the first copy.
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

/// Move clipboard text between the launcher session and the guest's console
/// (BSP's thread, at a host boundary). Returns whether bytes went into the
/// guest; their completion is signalled on `console_vmio`.
pub fn serve(
    session: &mut AppSession,
    console: &mut Console,
    console_vmio: &mut VirtioMmio,
    vm_id: usize,
) -> bool {
    let Some(port) = console.clipboard_port() else {
        return false;
    };
    if let Some(text) = port.take_from_guest() {
        session.clipboard_from_guest(&text);
    }
    if let Some(text) = session.take_clipboard_for_guest() {
        port.queue_for_guest(&text);
    }
    let delivered = console.flush_rx(&console_vmio.queue_cfg(0), vm_id);
    if delivered {
        console_vmio.signal_used();
    }
    delivered
}

/// The launcher's text named by `request`: copied out of its Grant only if
/// `client_tid` owns that Grant, and only if it is UTF-8. The length was
/// bounded by `VmmClipboardRequest::decode`.
pub fn from_launcher(client_tid: usize, request: &VmmClipboardRequest) -> Option<String> {
    let len = request.len as usize;
    let mut bytes = vec![0u8; len];
    let row = GrantRows {
        src_offset: 0,
        src_stride: len,
        dst_offset: 0,
        dst_stride: len,
        row_len: len,
        rows: 1,
    };
    sys_grant_copy_rows_owned_by(request.grant as usize, client_tid, row, &mut bytes)?;
    String::from_utf8(bytes).ok()
}
