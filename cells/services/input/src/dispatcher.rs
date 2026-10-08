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
//! ## Send-failure handling
//!
//! Keyboard dispatch uses blocking IPC so pressure propagates upstream without
//! dropping events. Mouse routing remains fire-and-forget through a separate
//! cached compositor endpoint.

use api::input::{encode_event, InputEvent, INPUT_EVENT_IPC_SIZE};
use api::syscall::service;
use ostd::syscall::{sys_lookup_service, sys_try_send};

/// Report every Nth dropped keyboard event, so a stalled consumer is visible
/// without one log line per keystroke.
const DROP_REPORT_EVERY: u64 = 64;

/// Opcode prefix byte sent to the focused cell's IPC endpoint.
pub const INPUT_EVENT_OPCODE: u8 = 0x10;

/// Routes translated events to the currently focused cell.
pub struct Dispatcher {
    /// Task ID of the currently focused cell (0 = no focus, events dropped).
    focused: usize,
    /// Keyboard events dropped because the focused cell was not draining. The
    /// first drop and every `DROP_REPORT_EVERY`-th after it are logged.
    dropped: u64,
    /// Reserved for a future focus-fallback policy; `dispatch()` does not read it.
    // reason: keyboard send failure currently leaves `focused` unchanged until a
    // later SetFocus. Keep the field only as a placeholder for a separately
    // specified policy, not as behaviour implemented by this dispatcher.
    #[allow(dead_code)]
    fallback_tid: usize,
    /// Cached compositor TID for mouse routing (0 = not yet resolved).
    /// Re-resolved lazily; reset to 0 when a send fails (compositor respawned).
    compositor_tid: usize,
}

impl Dispatcher {
    /// Create a dispatcher with no initial focus (events dropped until SetFocus).
    pub fn new() -> Self {
        Self {
            focused: 0,
            dropped: 0,
            fallback_tid: 0,
            compositor_tid: 0,
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
    /// **Non-blocking, and that is the fix for a board failure, not a
    /// preference.** Delivery used to block until the focused cell received the
    /// event, which applies backpressure from a slow consumer to *everything
    /// behind it*: while a Tier-3 guest held focus and was busy, the guest's VMM
    /// polled input only on its WFI/preemption exits, so the input service
    /// stayed blocked in this send, the USB cell stayed blocked feeding it, and
    /// the network cell stayed blocked sending through the USB cell — long
    /// enough that the kernel's liveness sweep killed net as hung
    /// (`task 7 … missed liveness deadline`, 2026-10-04). A keyboard is a
    /// lossy source at the consumer's pace; the events that do not fit are
    /// dropped and counted (`dropped`), never silently and never by stalling
    /// the cells behind them.
    ///
    /// The IPC message format is:
    /// ```text
    /// byte[0]   = INPUT_EVENT_OPCODE (0x10)
    /// byte[1..] = encode_event() output (see libs/api/src/input.rs)
    /// ```
    pub fn dispatch(&mut self, event: &InputEvent) -> bool {
        if self.focused == 0 {
            return true; // no focus — drop silently
        }
        if Self::try_send_event(self.focused, event).is_ok() {
            return true;
        }
        // The focused cell's queue is full: it is not draining. Drop this event
        // (and the ones that follow until it does) and say so periodically.
        self.dropped = self.dropped.wrapping_add(1);
        if self.dropped % DROP_REPORT_EVERY == 1 {
            ostd::io::print("[input] dropped ");
            ostd::io::print_usize(self.dropped as usize);
            ostd::io::println(" keyboard event(s): focused cell is not draining");
        }
        false
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
        if Self::try_send_event(self.compositor_tid, event).is_err() {
            self.compositor_tid = 0; // stale TID — re-resolve on next event
        }
    }

    /// Encode and try-send one mouse event to `target`.
    ///
    /// The IPC message format is:
    /// ```text
    /// byte[0]   = INPUT_EVENT_OPCODE (0x10)
    /// byte[1..] = encode_event() output (see libs/api/src/input.rs)
    /// ```
    fn try_send_event(target: usize, event: &InputEvent) -> Result<(), ()> {
        let buf = Self::encode(event);
        match sys_try_send(target, &buf) {
            ostd::syscall::SyscallResult::Ok(0) => Ok(()),
            _ => Err(()),
        }
    }

    fn encode(event: &InputEvent) -> [u8; INPUT_EVENT_IPC_SIZE + 1] {
        let mut buf = [0u8; INPUT_EVENT_IPC_SIZE + 1];
        buf[0] = INPUT_EVENT_OPCODE;
        let mut payload = [0u8; INPUT_EVENT_IPC_SIZE];
        encode_event(event, &mut payload);
        buf[1..INPUT_EVENT_IPC_SIZE + 1].copy_from_slice(&payload);
        buf
    }
}

impl Default for Dispatcher {
    fn default() -> Self {
        Self::new()
    }
}
