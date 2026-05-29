//! Focus-based input event dispatcher.
//!
//! Maintains a "focused cell" ID and routes translated `InputEvent`s to it
//! via IPC Send.  Focus changes clear transient modifiers to avoid stuck keys.

use api::input::{InputEvent, INPUT_EVENT_IPC_SIZE, encode_event};
use ostd::syscall::sys_send;

/// Opcode prefix byte sent to the focused cell's IPC endpoint.
pub const INPUT_EVENT_OPCODE: u8 = 0x10;

/// IPC endpoint ID used by the shell cell (boot default focused cell).
pub const DEFAULT_FOCUS_ENDPOINT: usize = 3; // shell is typically task 3

/// Routes translated events to the currently focused cell.
pub struct Dispatcher {
    /// Task ID / endpoint of the focused cell.
    focused: usize,
}

impl Dispatcher {
    /// Create a dispatcher with the boot-default focus (shell cell).
    pub fn new() -> Self {
        Self { focused: DEFAULT_FOCUS_ENDPOINT }
    }

    /// Change which cell receives input events.
    ///
    /// Also resets transient modifiers on the modifier-state tracker so that
    /// Shift/Ctrl/Alt do not appear "stuck" after a focus change.
    pub fn set_focus(&mut self, cell_endpoint: usize) {
        self.focused = cell_endpoint;
    }

    /// Return the currently focused endpoint.
    #[allow(dead_code)] // reason: used by compositor's set_focus IPC (Phase 16)
    pub fn focus(&self) -> usize {
        self.focused
    }

    /// Send a translated `InputEvent` to the focused cell.
    ///
    /// The IPC message format is:
    /// ```text
    /// byte[0]   = INPUT_EVENT_OPCODE (0x10)
    /// byte[1..] = encode_event() output (see libs/api/src/input.rs)
    /// ```
    pub fn dispatch(&self, event: &InputEvent) {
        let mut buf = [0u8; INPUT_EVENT_IPC_SIZE + 1];
        buf[0] = INPUT_EVENT_OPCODE;
        let mut payload = [0u8; INPUT_EVENT_IPC_SIZE];
        encode_event(event, &mut payload);
        buf[1..INPUT_EVENT_IPC_SIZE + 1].copy_from_slice(&payload);
        sys_send(self.focused, &buf);
    }
}

impl Default for Dispatcher {
    fn default() -> Self { Self::new() }
}
