//! CapId-keyed surface state table.
//!
//! Surface data, Grant-backed pixels, lifecycle transitions, and registry storage
//! live in focused private modules. This module preserves their compositor-facing
//! surface.

extern crate alloc;

mod configure;
pub mod frame;
mod lifecycle;
mod pixels;
mod state;
mod storage;

pub use configure::StateTransition;
pub use frame::FrameRegistry;
pub use state::SurfaceState;
pub use storage::SurfaceTable;

/// Maximum number of simultaneous surfaces.
pub const MAX_SURFACES: usize = 32;

/// Largest width or height a client may request. Keeps `w * h * bpp` and
/// `x + w` arithmetic far from `u32`/`i32` overflow on every target.
pub const MAX_SURFACE_DIM: u32 = 8192;
