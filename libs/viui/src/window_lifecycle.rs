//! Window lifecycle shared by every app with a compositor-managed window.
//!
//! The compositor resizes windows (user resize, maximize, Stack & Tile),
//! minimizes them and asks to close them. [`WindowLifecycle`] answers all of
//! that the same way for every app: a resize is applied to the surface (new
//! Grant, acknowledgement, old Grant released) before the app hears about it,
//! and a close request is answered by the app's [`ClosePolicy`]. The app's
//! loop only lays out and repaints at the size it is told.
//!
//! Single-window apps call [`WindowLifecycle::poll`] once per loop turn; it
//! drains every pending compositor event for the cell.

use wire::display::{WindowCloseRequest, WindowConfigure, WindowState};
use ostd::display::{poll_surface_events, SurfaceEvent, ViSurface};

/// How a compositor close request is answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClosePolicy {
    /// Keep the window open.
    Reject,
    /// Close: the request is accepted and the app is told [`WindowEvent::Closed`].
    Accept,
    /// Let the app decide (unsaved work): it is told
    /// [`WindowEvent::CloseRequested`] and answers with
    /// [`WindowLifecycle::respond_close`].
    Ask,
}

/// What an app's loop must act on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowEvent {
    /// The surface now has this size: lay out and repaint everything.
    Resized { width: u32, height: u32 },
    /// The window left the screen; painting can stop.
    Minimized,
    /// The window is back on screen: repaint everything.
    Restored,
    /// [`ClosePolicy::Ask`]: the user wants the window closed.
    CloseRequested { serial: u32 },
    /// The close was accepted and acknowledged: release the window and exit.
    Closed,
}

/// Lifecycle state of one compositor-managed window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowLifecycle {
    close_policy: ClosePolicy,
    minimized: bool,
    closed: bool,
}

impl WindowLifecycle {
    pub const fn new(close_policy: ClosePolicy) -> Self {
        Self { close_policy, minimized: false, closed: false }
    }

    pub fn set_close_policy(&mut self, close_policy: ClosePolicy) {
        self.close_policy = close_policy;
    }

    pub const fn is_minimized(&self) -> bool {
        self.minimized
    }

    /// Whether a close was accepted; the app should release its window.
    pub const fn is_closed(&self) -> bool {
        self.closed
    }

    /// Handle every pending compositor event for `surface`, reporting what
    /// the app must do through `on_event`.
    pub fn poll(&mut self, surface: &mut ViSurface, mut on_event: impl FnMut(WindowEvent)) {
        let cap = surface.cap();
        let surface = core::cell::RefCell::new(surface);
        for event in poll_surface_events(8) {
            let mut apply = |configure: WindowConfigure| {
                let mut surface = surface.borrow_mut();
                surface
                    .apply_configure(configure)
                    .ok()
                    .map(|()| (surface.width(), surface.height()))
            };
            let mut respond = |request: WindowCloseRequest, accept: bool| {
                surface.borrow().respond_close(request.serial, accept).is_ok()
            };
            self.handle_event(cap, event, &mut apply, &mut respond, &mut on_event);
        }
    }

    /// Answer a close request the app was asked about ([`ClosePolicy::Ask`]).
    /// Returns whether the window is now closing (accepted and delivered).
    pub fn respond_close(&mut self, surface: &ViSurface, serial: u32, accept: bool) -> bool {
        let delivered = surface.respond_close(serial, accept).is_ok();
        self.closed |= accept && delivered;
        self.closed
    }

    /// Handle one compositor event for the window `cap`. `apply` resizes the
    /// surface (new size, or `None` when the configure failed and the old
    /// buffer stays); `respond` answers a close request.
    pub(crate) fn handle_event(
        &mut self,
        cap: u32,
        event: SurfaceEvent,
        apply: &mut dyn FnMut(WindowConfigure) -> Option<(u32, u32)>,
        respond: &mut dyn FnMut(WindowCloseRequest, bool) -> bool,
        on_event: &mut dyn FnMut(WindowEvent),
    ) {
        match event {
            SurfaceEvent::Configure(configure) if configure.cap == cap => {
                if let Some((width, height)) = apply(configure) {
                    on_event(WindowEvent::Resized { width, height });
                }
            }
            SurfaceEvent::CloseRequest(request) if request.cap == cap => match self.close_policy {
                ClosePolicy::Ask => on_event(WindowEvent::CloseRequested { serial: request.serial }),
                policy => {
                    let accept = policy == ClosePolicy::Accept;
                    if respond(request, accept) && accept {
                        self.closed = true;
                        on_event(WindowEvent::Closed);
                    }
                }
            },
            SurfaceEvent::StateChanged(change) if change.cap == cap => {
                let minimized = change.state == WindowState::Minimized;
                if minimized != self.minimized {
                    self.minimized = minimized;
                    on_event(if minimized { WindowEvent::Minimized } else { WindowEvent::Restored });
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "window_lifecycle_tests.rs"]
mod tests;
