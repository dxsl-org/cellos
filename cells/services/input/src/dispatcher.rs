//! Focus-based input event dispatcher.
//!
//! Maintains a "focused cell" ID and routes translated `InputEvent`s to it
//! via IPC Send.  Focus changes clear transient modifiers to avoid stuck keys.
//!
//! ## Focus fallback (boot default)
//!
//! `fallback_tid` is set to `DEFAULT_FOCUS_ENDPOINT` at construction time.
//! This is fragile (assumes shell is task 3) and will be replaced with
//! `sys_lookup_service(service::SHELL)` once the shell registers itself as a
//! named service (requires a `service::SHELL` constant — future track).
//!
//! ## Death reversion
//!
//! `dispatch()` checks the `sys_send` return value.  When the focused cell has
//! exited, `sys_send` returns `Err(_)` and focus reverts to `fallback_tid`.
//! This avoids permanently dropping all key events after a UI cell exits.

use api::input::{InputEvent, INPUT_EVENT_IPC_SIZE, encode_event};
use ostd::syscall::{sys_send, SyscallResult};

/// Opcode prefix byte sent to the focused cell's IPC endpoint.
pub const INPUT_EVENT_OPCODE: u8 = 0x10;

/// Boot-default focus task ID.
///
/// Shell is spawned as the third task after init and VFS in the standard boot
/// order, making it TID 3.  This constant will be removed when the shell
/// registers itself via `sys_register_service(service::SHELL)`.
const DEFAULT_FOCUS_ENDPOINT: usize = 3;

/// Routes translated events to the currently focused cell.
pub struct Dispatcher {
    /// Task ID of the currently focused cell.
    focused: usize,
    /// Fallback TID used when the focused cell exits.  Initialised to the boot
    /// default (shell).  The first cell to explicitly call `SetFocus` does NOT
    /// update this — only `new()` sets it, so the shell always remains the fallback.
    fallback_tid: usize,
}

impl Dispatcher {
    /// Create a dispatcher with the boot-default focus (shell cell).
    pub fn new() -> Self {
        Self {
            focused: DEFAULT_FOCUS_ENDPOINT,
            fallback_tid: DEFAULT_FOCUS_ENDPOINT,
        }
    }

    /// Change which cell receives input events.
    ///
    /// Also resets transient modifiers on the modifier-state tracker so that
    /// Shift/Ctrl/Alt do not appear "stuck" after a focus change.
    pub fn set_focus(&mut self, cell_endpoint: usize) {
        self.focused = cell_endpoint;
    }

    /// Return the currently focused endpoint.
    #[allow(dead_code)]
    pub fn focus(&self) -> usize {
        self.focused
    }

    /// Send a translated `InputEvent` to the focused cell.
    ///
    /// If `sys_send` fails (focused cell has exited), focus reverts to
    /// `fallback_tid` so subsequent key events reach the shell again.
    ///
    /// The IPC message format is:
    /// ```text
    /// byte[0]   = INPUT_EVENT_OPCODE (0x10)
    /// byte[1..] = encode_event() output (see libs/api/src/input.rs)
    /// ```
    pub fn dispatch(&mut self, event: &InputEvent) {
        {
            use alloc::format;
            let msg = format!("[input-svc] dispatch to TID {}", self.focused);
            ostd::io::println(&msg);
        }
        let mut buf = [0u8; INPUT_EVENT_IPC_SIZE + 1];
        buf[0] = INPUT_EVENT_OPCODE;
        let mut payload = [0u8; INPUT_EVENT_IPC_SIZE];
        encode_event(event, &mut payload);
        buf[1..INPUT_EVENT_IPC_SIZE + 1].copy_from_slice(&payload);

        if let SyscallResult::Err(_) = sys_send(self.focused, &buf) {
            // Focused cell has exited — revert to fallback so keys reach the shell.
            if self.focused != self.fallback_tid {
                ostd::io::println("[input-svc] focused cell dead — reverting to fallback");
                self.focused = self.fallback_tid;
            }
        }
    }
}

impl Default for Dispatcher {
    fn default() -> Self { Self::new() }
}
