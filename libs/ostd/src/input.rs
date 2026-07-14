// SPDX-License-Identifier: MPL-2.0

//! Generic input event client for any Cell.
//!
//! Provides focus registration and non-blocking event polling without any
//! dependency on `libs/viui`. ViUI apps should use `viui::input_bridge`
//! which wraps this module and converts events to `viui::Event`.
//!
//! # Usage
//! ```no_run
//! use ostd::input::{request_focus, poll_events, InputEvent};
//!
//! // Once at startup:
//! while !request_focus() { ostd::task::yield_now(); }
//!
//! // Every tick:
//! for ev in poll_events(32) {
//!     if let InputEvent::Key(k) = ev { /* handle key */ }
//! }
//! ```

extern crate alloc;
use alloc::vec::Vec;

// Re-export api::input types so consumers can use `ostd::input::KeyState` etc.
pub use api::input::{InputEvent, KeyEvent, KeyState, KeySym, Modifiers, MouseButton};

use crate::syscall::{sys_lookup_service, sys_send, sys_try_recv, SyscallResult};
use api::{
    input::{decode_event, INPUT_EVENT_OPCODE},
    ipc::{InputRequest, IPC_BUF_SIZE},
    syscall::service,
};

/// Register this cell as the keyboard/mouse focus recipient.
///
/// Sends `InputRequest::SetFocus` to the input service. The service uses the
/// kernel-verified IPC sender TID — the `cell_tid` field is ignored, preventing
/// TID impersonation.
///
/// SetFocus is fire-and-forget: focus is granted atomically when the input service
/// receives the message. No reply is sent or awaited — a blocking reply caused a
/// scheduling race where input service ran before the caller entered sys_recv,
/// leaving a dangling send that blocked for the full watchdog interval (G18 fix).
///
/// Returns `true` when focus is sent. Returns `false` when the input service
/// is not yet registered (boot race) or `sys_send` fails (service dead).
pub fn request_focus() -> bool {
    let Some(input_tid) = sys_lookup_service(service::INPUT) else {
        return false;
    };

    let mut req_buf = [0u8; IPC_BUF_SIZE];
    let req = InputRequest::SetFocus { cell_tid: 0 };
    let Ok(encoded) = api::ipc::encode(&req, &mut req_buf) else {
        return false;
    };

    // Abort immediately if send fails (input service dead).
    !matches!(sys_send(input_tid, encoded), SyscallResult::Err(_))
}

/// Release keyboard/mouse focus from this cell.
///
/// Sends `InputRequest::ClearFocus` to the input service. The service clears
/// focus only if the sender is currently the focused cell (kernel-verified TID).
/// No-op if the input service is not registered or the send fails.
///
/// ClearFocus is fire-and-forget (same rationale as SetFocus). Call this before
/// blocking in `sys_wait` for a child cell, then re-request focus afterwards.
pub fn release_focus() {
    let Some(input_tid) = sys_lookup_service(service::INPUT) else {
        return;
    };
    let mut req_buf = [0u8; IPC_BUF_SIZE];
    let req = InputRequest::ClearFocus { cell_tid: 0 };
    let Ok(encoded) = api::ipc::encode(&req, &mut req_buf) else {
        return;
    };
    let _ = sys_send(input_tid, encoded);
}

/// Drain any pending IPC messages sent by the input service to this cell.
///
/// Shell reads UART via `sys_read(0)` (ring buffer) and also registers focus
/// with the input service (for VirtIO keyboard). The same keystroke arrives via
/// BOTH paths. After shell processes Enter from the UART path and exits
/// `read_line`, input service is blocked in `sys_send(shell, Enter_event)`
/// because shell is no longer in `sys_recv`. Call this before `release_focus()`
/// in `spawn_external` to drain via `sys_try_recv(input_tid)`, unblocking
/// input service so the subsequent `sys_send(input_tid, ClearFocus)` does not
/// deadlock (both would be in Sending state indefinitely).
pub fn drain_pending_input_events() {
    let Some(input_tid) = sys_lookup_service(service::INPUT) else {
        return;
    };
    let mut buf = [0u8; IPC_BUF_SIZE];
    // mask=input_tid: only drain messages from the input service (not from
    // other cells that may have sent to us).  Loop until queue empty (Ok(0)).
    for _ in 0..32 {
        match sys_try_recv(input_tid, &mut buf) {
            SyscallResult::Ok(0) => break,
            SyscallResult::Ok(_) => {}
            SyscallResult::Err(_) => break,
        }
    }
}

/// Non-blocking drain of pending input events (up to `max` events).
///
/// Calls `sys_try_recv` in a loop; stops when the queue is empty or `max` is
/// reached. Messages from senders other than the input service are discarded.
/// Safe to call every frame — returns an empty `Vec` during the boot race when
/// the input service is not yet registered.
pub fn poll_events(max: usize) -> Vec<InputEvent> {
    let Some(input_tid) = sys_lookup_service(service::INPUT) else {
        return Vec::new();
    };

    let mut events = Vec::with_capacity(max.min(16));
    while events.len() < max {
        let mut buf = [0u8; 65];
        // Mask on the input service TID: matches both the Sending-scan and the
        // pending_msgs drain in the kernel's TryRecv handler, and — crucially —
        // leaves non-input IPC (e.g. compositor replies) queued rather than
        // consuming and discarding it. A wildcard mask (0 or usize::MAX) would
        // either eat unrelated messages or match nothing queued.
        match sys_try_recv(input_tid, &mut buf) {
            SyscallResult::Ok(0) => break, // queue empty
            SyscallResult::Ok(sender) if sender == input_tid => {
                if let Some(ev) = parse_frame(&buf) {
                    events.push(ev);
                }
            }
            SyscallResult::Ok(_) => {} // unexpected sender — discard
            SyscallResult::Err(_) => break,
        }
    }
    events
}

/// Decode a 65-byte input-service IPC message into an `InputEvent`.
///
/// Returns `None` for messages with a wrong opcode or unsupported discriminant.
/// Exposed as `pub(crate)` so `ostd::app` can reuse it without re-exporting.
pub(crate) fn parse_frame(buf: &[u8]) -> Option<InputEvent> {
    if buf.len() < 2 || buf[0] != INPUT_EVENT_OPCODE {
        return None;
    }
    decode_event(&buf[1..])
}
