//! App-side display helpers: `ViSurface` for Grant-backed compositor surfaces.
//!
//! ## Usage
//! ```no_run
//! use ostd::display::{wait_for_compositor, ViSurface};
//! use ostd::services::display::PixelFormat;
//!
//! # fn create_surface() -> ostd::ViResult<()> {
//! let comp_tid = wait_for_compositor();
//! let mut surface = ViSurface::create(comp_tid, 640, 480, PixelFormat::Bgra8888)?;
//! let pixels = surface.pixels_mut();
//! // draw into pixels ...
//! surface.damage_all();
//! # Ok(())
//! # }
//! ```

mod dispatcher;
mod events;
mod ipc;
mod lifecycle;
mod routing;
mod surface;

pub use events::{SurfaceEvent, MAX_SURFACE_EVENTS, MAX_SURFACE_EVENT_CAPS};
pub use routing::poll_surface_events;
pub use surface::ViSurface;

pub(crate) use dispatcher::{route_compositor_frame, take_forwarded_input_event};
pub(crate) use routing::drain_compositor_once;

use crate::syscall::sys_lookup_service;
use api::syscall::service;

/// Block until the compositor service is registered and return its TID.
pub fn wait_for_compositor() -> usize {
    loop {
        if let Some(tid) = sys_lookup_service(service::COMPOSITOR) {
            return tid;
        }
        crate::task::yield_now();
    }
}
