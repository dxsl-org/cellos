//! App-side display helpers: `ViSurface` for Grant-backed compositor surfaces.
//!
//! ## Usage
//! ```
//! let comp_tid = wait_for_compositor();
//! let mut surf = ViSurface::create(comp_tid, 640, 480, PixelFormat::Bgra8888)?;
//! let px = surf.pixels_mut();
//! // draw into px ...
//! surf.damage_all();
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
