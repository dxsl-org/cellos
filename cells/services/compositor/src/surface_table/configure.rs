use api::display::{ConfigureKind, PixelFormat, Rect, WindowConfigure, WindowState};

use super::{state::PixelSource, SurfaceState};

/// An uncommitted buffer and geometry proposed to a window owner.
pub(super) struct PendingConfigure {
    pub(super) serial: u32,
    pub(super) kind: ConfigureKind,
    pub(super) rect: Rect,
    pub(super) staged_format: Option<PixelFormat>,
    pub(super) staged_source: Option<PixelSource>,
}

/// Result of accepting a state request.
pub enum StateTransition {
    StateChanged(api::display::WindowStateChanged),
    Configure(WindowConfigure),
}

impl SurfaceState {
    /// Begin a window-manager state request. Background caps and closing/pending
    /// windows reject every request without mutating the active presentation.
    pub fn request_state(
        &mut self,
        cap: u32,
        opcode: u8,
        maximum_rect: Rect,
    ) -> Option<StateTransition> {
        use api::display::compositor_ops;

        if !self.is_window_managed()
            || self.state == WindowState::Closing
            || self.pending_configure.is_some()
        {
            return None;
        }
        match opcode {
            compositor_ops::MINIMIZE
                if matches!(self.state, WindowState::Normal | WindowState::Maximized) =>
            {
                self.minimized_from = self.state;
                self.state = WindowState::Minimized;
                Some(StateTransition::StateChanged(self.state_changed(cap)))
            }
            compositor_ops::MAXIMIZE if self.state == WindowState::Normal => self
                .begin_configure(cap, ConfigureKind::Maximize, maximum_rect)
                .map(StateTransition::Configure),
            compositor_ops::RESTORE if self.state == WindowState::Minimized => {
                self.state = self.minimized_from;
                Some(StateTransition::StateChanged(self.state_changed(cap)))
            }
            compositor_ops::RESTORE if self.state == WindowState::Maximized => self
                .begin_configure(cap, ConfigureKind::Restore, self.normal_rect)
                .map(StateTransition::Configure),
            _ => None,
        }
    }

    /// Propose a resize to a visible interactive surface.
    pub fn begin_configure(
        &mut self,
        cap: u32,
        kind: ConfigureKind,
        rect: Rect,
    ) -> Option<WindowConfigure> {
        if !self.is_window_managed()
            || !self.is_visible_for_paint()
            || self.pending_configure.is_some()
            || self.retired_grant_id.is_some()
            || rect.w == 0
            || rect.h == 0
        {
            return None;
        }
        let serial = self.next_serial();
        self.pending_configure = Some(PendingConfigure {
            serial,
            kind,
            rect,
            staged_format: None,
            staged_source: None,
        });
        Some(WindowConfigure {
            opcode: api::display::compositor_events::WINDOW_CONFIGURE,
            kind,
            _pad: [0; 2],
            cap,
            serial,
            rect,
        })
    }

    /// Discard an undeliverable configure so a later proposal can proceed.
    pub fn cancel_configure(&mut self, serial: u32) {
        if self
            .pending_configure
            .as_ref()
            .map(|pending| pending.serial)
            == Some(serial)
        {
            self.pending_configure = None;
        }
    }

    /// Restore the state before a state-change event that could not be delivered.
    pub fn rollback_state_change(&mut self, state: WindowState) {
        self.state = state;
    }

    /// Commit a fully staged configure only when the owner echoes its live serial.
    pub fn acknowledge_configure(
        &mut self,
        cap: u32,
        serial: u32,
    ) -> Option<(Rect, Rect, api::display::WindowStateChanged)> {
        let mut pending = self.pending_configure.take()?;
        if pending.serial != serial {
            self.pending_configure = Some(pending);
            return None;
        }
        let Some(source) = pending.staged_source.take() else {
            self.pending_configure = Some(pending);
            return None;
        };
        let retired_grant_id = match &self.source {
            PixelSource::Grant { reg_id, .. } => Some(*reg_id),
            PixelSource::Owned(_) => None,
        };
        let Some(fmt) = pending.staged_format.take() else {
            pending.staged_source = Some(source);
            self.pending_configure = Some(pending);
            return None;
        };
        let old_rect = self.screen_rect();
        self.x = pending.rect.x;
        self.y = pending.rect.y;
        self.w = pending.rect.w;
        self.retired_grant_id = retired_grant_id;
        self.h = pending.rect.h;
        self.fmt = fmt;
        self.source = source;
        match pending.kind {
            ConfigureKind::Maximize => self.state = WindowState::Maximized,
            ConfigureKind::Restore | ConfigureKind::Resize => {
                self.state = WindowState::Normal;
                self.normal_rect = pending.rect;
            }
        }
        self.damage = Some(Rect {
            x: 0,
            y: 0,
            w: self.w,
            h: self.h,
        });
        let state = self.state_changed(cap);
        Some((old_rect, self.screen_rect(), state))
    }
}
