//! CapId-keyed surface state table.
//!
//! Surface data, Grant-backed pixels, lifecycle transitions, and registry storage
//! live in focused private modules. This module preserves their compositor-facing
//! surface.

extern crate alloc;

mod configure;
mod lifecycle;
mod pixels;
mod state;
mod storage;

pub use configure::StateTransition;
pub use state::SurfaceState;
pub use storage::SurfaceTable;

/// Maximum number of simultaneous surfaces.
pub const MAX_SURFACES: usize = 32;
