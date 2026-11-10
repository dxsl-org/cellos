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

// Every blit leases its surface's Grant until the next receive, and one render
// pass blits each surface's single active Grant. The kernel refuses a holder's
// 65th distinct lease as an unknown id, which `resolve_pixels` would treat as a
// released buffer and detach, so every surface must fit one holder's table.
const _: () = assert!(MAX_SURFACES <= api::syscall::grant_slice::MAX_LEASES_PER_HOLDER);

/// Largest width or height a client may request. Keeps `w * h * bpp` and
/// `x + w` arithmetic far from `u32`/`i32` overflow on every target.
pub const MAX_SURFACE_DIM: u32 = 8192;
