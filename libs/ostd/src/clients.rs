// SPDX-License-Identifier: MPL-2.0

//! Ergonomic typed client facades for ViCell system services.
//!
//! Each client wraps a [`ServiceRef`][crate::service::ServiceRef] and hides the
//! low-level request-construction and postcard serialization behind named methods.
//!
//! # Usage
//! ```no_run
//! use ostd::prelude::*;
//!
//! fn handler(ctx: &mut AppContext, ev: AppEvent) {
//!     if let AppEvent::Init = ev {
//!         let data = ctx.vfs().read_file("/etc/hostname").unwrap_or_default();
//!     }
//! }
//! ```
//!
//! Clients are lazily initialized via [`AppContext::vfs`], [`AppContext::net`],
//! and [`AppContext::input`].  They can also be constructed standalone:
//! ```no_run
//! let mut vfs = ostd::clients::VfsClient::new();
//! let bytes = vfs.read_file("/bin/shell")?;
//! ```

pub mod input;
pub mod net;
pub mod vfs;

/// TLS embedded_io stream adapter (only when `http`/`json` features pull it in).
///
/// NOTE on the "shared write-all helper" plan ask: `TcpStream::write` (net.rs)
/// deliberately does a SINGLE chunked send and returns the chunk length, leaving
/// write-all to `embedded_io::Write::write_all`. `TlsStream::write` instead
/// loops internally (write-all semantics) because the raw `tls_write` cap is
/// 503 B and partial. Pushing that loop into `TcpStream::write` would change its
/// public return contract (callers expect a single-chunk count), a behavior
/// regression — so the loop stays local to `TlsStream`. They share the *pattern*
/// (chunk-cap + yield), not a function, by design.
#[cfg(any(feature = "http", feature = "json"))]
pub mod tls_stream;

pub use input::InputClient;
pub use net::{NetClient, TcpStream};
#[cfg(any(feature = "http", feature = "json"))]
pub use tls_stream::TlsStream;
pub use vfs::VfsClient;

// ── Shared helper ─────────────────────────────────────────────────────────────

/// Convert a `VfsResponse::Err` / `NetResponse::Err` discriminant byte to a `ViError`.
///
/// `ViError` is `#[repr(C)]` with sequential discriminants from 0.
pub(crate) fn vierr_from_code(code: u8) -> crate::ViError {
    use crate::ViError;
    match code {
        0 => ViError::OutOfMemory,
        1 => ViError::InvalidArgument,
        2 => ViError::NotFound,
        3 => ViError::PermissionDenied,
        4 => ViError::AlreadyExists,
        5 => ViError::WouldBlock,
        6 => ViError::NotSupported,
        7 => ViError::IO,
        8 => ViError::InvalidInput,
        9 => ViError::IsADirectory,
        10 => ViError::NotADirectory,
        _ => ViError::Unknown,
    }
}
