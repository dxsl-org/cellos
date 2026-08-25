use api::display::{CloseResponseAction, Rect, WindowCloseRequest, WindowStateChanged};

use super::SurfaceState;

/// A close request awaiting an owner response.
pub(super) struct PendingClose {
    pub(super) serial: u32,
    pub(super) restore_state: api::display::WindowState,
}

impl SurfaceState {
    /// Start a close handshake without destroying the surface or its source mapping.
    pub fn begin_close(
        &mut self,
        cap: u32,
    ) -> Option<(Rect, WindowCloseRequest, WindowStateChanged)> {
        if !self.is_window_managed()
            || self.state == api::display::WindowState::Closing
            || self.pending_configure.is_some()
        {
            return None;
        }
        let old_rect = self.screen_rect();
        let restore_state = self.state;
        let close_serial = self.next_serial();
        self.pending_close = Some(PendingClose {
            serial: close_serial,
            restore_state,
        });
        self.state = api::display::WindowState::Closing;
        let close = WindowCloseRequest {
            opcode: api::display::compositor_events::WINDOW_CLOSE_REQUEST,
            _pad: [0; 3],
            cap,
            serial: close_serial,
        };
        let state = self.state_changed(cap);
        Some((old_rect, close, state))
    }

    /// Restore the state before an undeliverable close request.
    pub fn cancel_close(&mut self, serial: u32) {
        let Some(pending) = self.pending_close.take() else {
            return;
        };
        if pending.serial == serial {
            self.state = pending.restore_state;
        } else {
            self.pending_close = Some(pending);
        }
    }

    /// Process an owner close response. Acceptance intentionally leaves the surface
    /// closing: the owner retains and must explicitly detach/destroy its source.
    pub fn close_response(
        &mut self,
        cap: u32,
        serial: u32,
        action: CloseResponseAction,
    ) -> Option<WindowStateChanged> {
        let pending = self.pending_close.take()?;
        if pending.serial != serial {
            self.pending_close = Some(pending);
            return None;
        }
        if action == CloseResponseAction::Accept {
            return None;
        }
        self.state = pending.restore_state;
        Some(self.state_changed(cap))
    }

    pub(super) fn next_serial(&mut self) -> u32 {
        self.serial = self.serial.wrapping_add(1);
        if self.serial == 0 {
            self.serial = 1;
        }
        self.serial
    }

    pub(super) fn state_changed(&mut self, cap: u32) -> WindowStateChanged {
        WindowStateChanged {
            opcode: api::display::compositor_events::WINDOW_STATE_CHANGED,
            state: self.state,
            _pad: [0; 2],
            cap,
            serial: self.next_serial(),
        }
    }
}
