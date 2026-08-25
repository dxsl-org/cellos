//! Public compositor lifecycle event types.

use api::display::{WindowCloseRequest, WindowConfigure, WindowStateChanged};

/// A typed lifecycle event sent by the trusted compositor to a surface owner.
///
/// These events are retained in a bounded, allocation-free dispatcher until
/// [`crate::display::poll_surface_events`] returns them.
#[repr(C, u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceEvent {
    /// The compositor proposes a new content rectangle.
    Configure(WindowConfigure),
    /// The compositor asks the owner to accept or reject a close.
    CloseRequest(WindowCloseRequest),
    /// The compositor reports a managed-state transition.
    StateChanged(WindowStateChanged),
}

/// Maximum number of surface capabilities retained by the lifecycle dispatcher.
pub const MAX_SURFACE_EVENT_CAPS: usize = 32;

/// Maximum lifecycle events that can be retained at once.
pub const MAX_SURFACE_EVENTS: usize = MAX_SURFACE_EVENT_CAPS * 3;
