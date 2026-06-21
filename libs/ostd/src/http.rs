// SPDX-License-Identifier: MPL-2.0

//! `ostd::http` — feature-gated HTTP/1.1 client surface (`feature = "http"`).
//!
//! Re-exports the pure protocol logic from [`http_core`] (host-testable) and
//! adds the ostd-only transport glue: [`HttpClient`] (generic over any
//! `embedded_io::Read + Write`) and [`TlsStream`] (HTTPS adapter over the net
//! cell's raw TLS IPC).
//!
//! ```no_run
//! use ostd::http::{HttpClient, TlsStream};
//! use ostd::json;
//!
//! let tls = TlsStream::connect([10, 0, 2, 2], 8443, "mock")?;
//! let mut client = HttpClient::new(tls);
//! let (headers, mut body) = client.post(
//!     "mock", "/v1/chat/completions", "application/json", br#"{"q":"hi"}"#,
//! )?;
//! // drain `body` via body.read(&mut transport, &mut out) ...
//! # Ok::<(), http_core::HttpError>(())
//! ```

#[path = "http/client.rs"]
mod client;

pub use client::HttpClient;

/// HTTPS transport adapter (TLS over the net cell). Re-exported here so callers
/// only need `use ostd::http::{HttpClient, TlsStream}`.
pub use crate::clients::TlsStream;

// ── Re-exports from the pure, host-tested protocol crate ───────────────────────
pub use http_core::request::RequestBuilder;
pub use http_core::response::parse_response_headers;
pub use http_core::{BodyReader, Framing, HttpError, ParsedHeaders};
