//! Focus-based input event dispatcher.
//!
//! Maintains a "focused cell" ID and routes translated `InputEvent`s to it
//! via IPC Send.  Focus changes clear transient modifiers to avoid stuck keys.
//!
//! ## Focus default
//!
//! `focused` starts at 0 (no focus).  The first cell to call `SetFocus` owns
//! the keyboard.  Events before any focus claim are silently dropped — this is
//! preferable to the previous TID-3 shell fallback which consumed events without
//! the shell ever reading them (shell uses sys_read(0), not the input service).
//!
//! ## Death reversion
//!
//! `dispatch()` checks the `sys_send` return value.  When the focused cell has
//! exited, `sys_send` returns `Err(_)` and focus reverts to 0.  The next app
//! that calls `SetFocus` resumes delivery.

use api::input::{InputEvent, INPUT_EVENT_IPC_SIZE, encode_event};
use api::syscall::service;
use ostd::syscall::{sys_lookup_service, sys_try_send};


/// Opcode prefix byte sent to the focused cell's IPC endpoint.
pub const INPUT_EVENT_OPCODE: u8 = 0x10;

/// Routes translated events to the currently focused cell.
pub struct Dispatcher {
    /// Task ID of the currently focused cell (0 = no focus, events dropped).
    focused: usize,
    /// Fallback TID on focus-cell death (0 = park until next SetFocus).
    fallback_tid: usize,
    /// Cached compositor TID for mouse routing (0 = not yet resolved).
    /// Re-resolved lazily; reset to 0 when a send fails (compositor respawned).
    compositor_tid: usize,
}

impl Dispatcher {
    /// Create a dispatcher with no initial focus (events dropped until SetFocus).
    pub fn new() -> Self {
        Self { focused: 0, fallback_tid: 0, compositor_tid: 0 }
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
        if self.focused == 0 {
            return; // no focus — drop silently
        }
        let _ = Self::send_event(self.focused, event);
        // NOTE: no per-dispatch logging — it would print a line on the shared
        // console for every keystroke, burying the shell prompt the user types at.
    }

    /// Send a mouse event (`MouseMove`/`MouseButton`/`MouseScroll`) to the
    /// compositor rather than the keyboard-focused cell.
    ///
    /// The compositor owns the cursor and the surface Z-order, so it is the
    /// only correct recipient for pointer events: it repaints/moves the cursor
    /// and hit-tests button clicks to the surface under the pointer. Routing
    /// mouse through the keyboard focus (the historical behaviour) breaks as
    /// soon as a non-GUI cell like the shell holds focus — the events land in
    /// a cell that ignores them and the cursor never moves.
    ///
    /// The compositor TID is resolved lazily via the service registry and
    /// re-resolved after a send failure (compositor death/respawn).
    pub fn dispatch_mouse(&mut self, event: &InputEvent) {
        if self.compositor_tid == 0 {
            match sys_lookup_service(service::COMPOSITOR) {
                Some(tid) => self.compositor_tid = tid,
                None => return, // compositor not up yet — drop
            }
        }
        if Self::send_event(self.compositor_tid, event).is_err() {
            self.compositor_tid = 0; // stale TID — re-resolve on next event
        }
    }

    /// Encode and try-send one event to `target`.
    ///
    /// The IPC message format is:
    /// ```text
    /// byte[0]   = INPUT_EVENT_OPCODE (0x10)
    /// byte[1..] = encode_event() output (see libs/api/src/input.rs)
    /// ```
    fn send_event(target: usize, event: &InputEvent) -> Result<(), ()> {
        let mut buf = [0u8; INPUT_EVENT_IPC_SIZE + 1];
        buf[0] = INPUT_EVENT_OPCODE;
        let mut payload = [0u8; INPUT_EVENT_IPC_SIZE];
        encode_event(event, &mut payload);
        buf[1..INPUT_EVENT_IPC_SIZE + 1].copy_from_slice(&payload);

        // Non-blocking dispatch: if the target is not receiving (and its
        // pending_msgs queue is full), the event is dropped.
        match sys_try_send(target, &buf) {
            ostd::syscall::SyscallResult::Ok(_) => Ok(()),
            _ => Err(()),
        }
    }
}

impl Default for Dispatcher {
    fn default() -> Self { Self::new() }
}
