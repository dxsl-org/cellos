//! Lifecycle adapter for one compositor-managed ViUI surface.
//!
//! The adapter owns a [`crate::app_runner::ViApp`] and handles compositor
//! events with [`WindowLifecycle`] before each tick. A resize marks the app for
//! a full relayout; a failed configure leaves the previous Grant and layout.

extern crate alloc;

use alloc::{boxed::Box, rc::Rc};
use api::display::{WindowCloseRequest, WindowConfigure};
use ostd::display::{poll_surface_events, ViSurface};

use crate::app_runner::ViApp;
use crate::event::Event;
use crate::node::ViNode;
use crate::renderer::{shared_surface, FramebufferRenderer, SurfaceHandle};
pub use crate::window_lifecycle::ClosePolicy;
use crate::window_lifecycle::{WindowEvent, WindowLifecycle};

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
    lifecycle: WindowLifecycle,
}

impl ManagedSurfaceApp {
    /// Create a managed ViUI application with the default close-reject policy.
    pub fn new(root: Box<dyn ViNode>, surface: ViSurface) -> Self {
        let surface = shared_surface(surface);
        let renderer = FramebufferRenderer::from_surface_handle(surface.clone());
        Self {
            app: ViApp::new(root, Box::new(renderer)),
            surface,
            lifecycle: WindowLifecycle::new(ClosePolicy::Reject),
        }
    }

    /// Select the response sent for matching compositor close requests.
    /// [`ClosePolicy::Ask`] is not offered here: there is no app loop to ask.
    pub fn set_close_policy(&mut self, close_policy: ClosePolicy) {
        let policy = if close_policy == ClosePolicy::Ask { ClosePolicy::Reject } else { close_policy };
        self.lifecycle.set_close_policy(policy);
    }

    /// Return the app for configuration that must happen before ticking.
    pub fn app_mut(&mut self) -> &mut ViApp {
        &mut self.app
    }

    /// Return whether a matching close request was accepted.
    pub const fn is_closed(&self) -> bool {
        self.lifecycle.is_closed()
    }

    /// Poll lifecycle events and process input using zero elapsed animation time.
    pub fn tick(&mut self, events: &[Event]) -> ManagedTick {
        self.tick_with_dt(events, 0)
    }

    /// Poll lifecycle events and advance the contained app by `dt_ms`.
    pub fn tick_with_dt(&mut self, events: &[Event], dt_ms: u32) -> ManagedTick {
        self.process_lifecycle_events();
        if self.lifecycle.is_closed() {
            return ManagedTick::Closed;
        }
        if self.lifecycle.is_minimized() {
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
            let surface = &self.surface;
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
            let mut relayout = false;
            self.lifecycle.handle_event(cap, event, &mut apply, &mut respond, &mut |event| {
                relayout |= matches!(event, WindowEvent::Resized { .. } | WindowEvent::Restored);
            });
            if relayout {
                self.app.mark_dirty();
            }
        }
    }
}
