//! Lifecycle adapter for one compositor-managed ViUI surface.
//!
//! The adapter owns a [`crate::app_runner::ViApp`] and polls compositor events
//! before each tick. A successful configure marks the app for a full relayout;
//! failed configure transactions leave the previous Grant and layout active.

extern crate alloc;

use alloc::{boxed::Box, rc::Rc};
use api::display::{WindowCloseRequest, WindowConfigure, WindowState};
use ostd::display::{poll_surface_events, SurfaceEvent, ViSurface};

use crate::app_runner::ViApp;
use crate::event::Event;
use crate::node::ViNode;
use crate::renderer::{shared_surface, FramebufferRenderer, SurfaceHandle};

/// Decision returned to the compositor for every matching close request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClosePolicy {
    /// Keep the surface alive after a close request.
    Reject,
    /// Permit the compositor to close the surface.
    Accept,
}

/// Result of advancing a managed surface by one tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManagedTick {
    /// A frame was rendered and submitted.
    Rendered,
    /// No frame was needed or the surface is minimized.
    Idle,
    /// A close request was accepted and acknowledged.
    Closed,
}

/// One ViUI application connected to one compositor-managed surface.
pub struct ManagedSurfaceApp {
    app: ViApp,
    surface: SurfaceHandle,
    lifecycle: LifecycleController,
}

impl ManagedSurfaceApp {
    /// Create a managed ViUI application with the default close-reject policy.
    pub fn new(root: Box<dyn ViNode>, surface: ViSurface) -> Self {
        let surface = shared_surface(surface);
        let renderer = FramebufferRenderer::from_surface_handle(surface.clone());
        Self {
            app: ViApp::new(root, Box::new(renderer)),
            surface,
            lifecycle: LifecycleController::new(),
        }
    }

    /// Select the response sent for matching compositor close requests.
    pub fn set_close_policy(&mut self, close_policy: ClosePolicy) {
        self.lifecycle.close_policy = close_policy;
    }

    /// Return the app for configuration that must happen before ticking.
    pub fn app_mut(&mut self) -> &mut ViApp {
        &mut self.app
    }

    /// Return whether a matching close request was accepted.
    pub const fn is_closed(&self) -> bool {
        self.lifecycle.closed
    }

    /// Poll lifecycle events and process input using zero elapsed animation time.
    pub fn tick(&mut self, events: &[Event]) -> ManagedTick {
        self.tick_with_dt(events, 0)
    }

    /// Poll lifecycle events and advance the contained app by `dt_ms`.
    pub fn tick_with_dt(&mut self, events: &[Event], dt_ms: u32) -> ManagedTick {
        self.process_lifecycle_events();
        if self.lifecycle.closed {
            return ManagedTick::Closed;
        }
        if self.lifecycle.minimized {
            return ManagedTick::Idle;
        }
        if self.app.tick_with_dt(events, dt_ms) {
            ManagedTick::Rendered
        } else {
            ManagedTick::Idle
        }
    }

    /// Consume a closed managed app and release its compositor surface.
    ///
    /// Call this after [`ManagedTick::Closed`] before dropping the app. The
    /// contained renderer is dropped first, leaving this adapter as the only
    /// surface owner; it then sends the normal `ViSurface` destruction sequence.
    pub fn shutdown(self) {
        let Self { app, surface, .. } = self;
        drop(app);
        match Rc::try_unwrap(surface) {
            Ok(surface) => surface.into_inner().destroy(),
            Err(_) => unreachable!("managed surface handles are crate-private"),
        }
    }

    fn process_lifecycle_events(&mut self) {
        let cap = self.surface.borrow().cap();
        for event in poll_surface_events(8) {
            let mut apply = |configure: WindowConfigure| {
                self.surface.borrow_mut().apply_configure(configure).is_ok()
            };
            let mut respond = |request: WindowCloseRequest, accept: bool| {
                self.surface
                    .borrow()
                    .respond_close(request.serial, accept)
                    .is_ok()
            };
            if self
                .lifecycle
                .handle_event(cap, event, &mut apply, &mut respond)
            {
                self.app.mark_dirty();
            }
        }
    }
}

struct LifecycleController {
    close_policy: ClosePolicy,
    minimized: bool,
    closed: bool,
}

impl LifecycleController {
    const fn new() -> Self {
        Self {
            close_policy: ClosePolicy::Reject,
            minimized: false,
            closed: false,
        }
    }

    /// Returns whether the app must relayout and repaint its full surface.
    fn handle_event(
        &mut self,
        cap: u32,
        event: SurfaceEvent,
        apply: &mut dyn FnMut(WindowConfigure) -> bool,
        respond: &mut dyn FnMut(WindowCloseRequest, bool) -> bool,
    ) -> bool {
        match classify_event(cap, event) {
            LifecycleAction::Ignore => false,
            LifecycleAction::Configure(configure) => apply(configure),
            LifecycleAction::Close(request) => {
                let accept = self.close_policy == ClosePolicy::Accept;
                let delivered = respond(request, accept);
                self.closed = accept && delivered;
                false
            }
            LifecycleAction::State(minimized) => {
                let restored = self.minimized && !minimized;
                self.minimized = minimized;
                restored
            }
        }
    }
}

enum LifecycleAction {
    Ignore,
    Configure(WindowConfigure),
    Close(WindowCloseRequest),
    State(bool),
}

fn classify_event(cap: u32, event: SurfaceEvent) -> LifecycleAction {
    match event {
        SurfaceEvent::Configure(configure) if configure.cap == cap => {
            LifecycleAction::Configure(configure)
        }
        SurfaceEvent::CloseRequest(request) if request.cap == cap => {
            LifecycleAction::Close(request)
        }
        SurfaceEvent::StateChanged(change) if change.cap == cap => {
            LifecycleAction::State(change.state == WindowState::Minimized)
        }
        _ => LifecycleAction::Ignore,
    }
}

#[cfg(test)]
#[path = "managed_surface_tests.rs"]
mod tests;
