// SPDX-License-Identifier: MPL-2.0

//! `HttpClient<T>` — drives an HTTP/1.1 request/response exchange over any
//! blocking [`embedded_io::Read`] + [`embedded_io::Write`] transport.
//!
//! Works identically over [`TcpStream`][crate::clients::TcpStream] (plaintext
//! HTTP) and [`TlsStream`][crate::clients::TlsStream] (HTTPS) — that generic
//! transport is the whole point of the embedded_io adapters.
//!
//! # Exchange contract (what completes before what)
//! 1. [`send`][HttpClient::send] writes the full request bytes via `write_all`
//!    (the transport handles its own chunking).
//! 2. It then reads response bytes into a growing buffer, calling
//!    [`parse_response_headers`] after each read until it returns `Ok` (headers
//!    complete). A `NeedMoreData` means "keep accumulating".
//! 3. A [`BodyReader`] is constructed seeded with the post-header bytes already
//!    in the buffer (`buf[body_offset..]`) — CRITICAL because the first chunk or
//!    a short Content-Length body often arrives in the SAME packet as the
//!    headers. The caller then drains the body via `reader.read(transport, ..)`.

extern crate alloc;

use alloc::vec::Vec;
use embedded_io::{Read, Write};
use http_core::request::RequestBuilder;
use http_core::response::parse_response_headers;
use http_core::{BodyReader, HttpError, ParsedHeaders};

/// Initial response accumulation buffer capacity (one IPC-sized read).
const INITIAL_RECV: usize = 4096;

/// Hard cap on accumulated *header* bytes before giving up, guarding against a
/// hostile server that never terminates the header block. The body is NOT
/// buffered here (that is `BodyReader`'s streaming job) — only headers.
const MAX_HEADER_BYTES: usize = 64 * 1024;

/// A generic blocking HTTP/1.1 client over a `Read + Write` transport.
pub struct HttpClient<T> {
    transport: T,
}

impl<T: Read + Write> HttpClient<T> {
    /// Wrap an already-connected transport.
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

    /// Consume the client and return the underlying transport (e.g. to close it).
    pub fn into_inner(self) -> T {
        self.transport
    }

    /// Send a pre-built request and parse the response headers.
    ///
    /// Returns the parsed headers plus a [`BodyReader`] seeded with any body
    /// bytes that arrived alongside the headers. Drain the body by repeatedly
    /// calling [`BodyReader::read`] with [`transport`][HttpClient::transport].
    ///
    /// # Errors
    /// - Transport write/read failure → `HttpError::Other`.
    /// - Headers never complete within [`MAX_HEADER_BYTES`] → `HttpError::Other`.
    /// - Malformed status/headers → propagated from [`parse_response_headers`].
    pub fn send(&mut self, req: &[u8]) -> Result<(ParsedHeaders, BodyReader), HttpError> {
        self.transport
            .write_all(req)
            .map_err(|_| HttpError::Other(alloc::string::String::from("transport write failed")))?;
        self.transport
            .flush()
            .map_err(|_| HttpError::Other(alloc::string::String::from("transport flush failed")))?;

        let mut buf: Vec<u8> = Vec::with_capacity(INITIAL_RECV);
        let mut tmp = [0u8; INITIAL_RECV];

        loop {
            // Try to parse what we have. The first iteration may already hold a
            // complete header block if the request and response raced; but with
            // an empty buf parse returns NeedMoreData, so read first below.
            match parse_response_headers(&buf) {
                Ok(headers) => {
                    let body = &buf[headers.body_offset..];
                    let reader = BodyReader::new(headers.framing, headers.content_length, body);
                    return Ok((headers, reader));
                }
                Err(HttpError::NeedMoreData) => { /* fall through to read more */ }
                Err(e) => return Err(e),
            }

            if buf.len() >= MAX_HEADER_BYTES {
                return Err(HttpError::Other(alloc::string::String::from(
                    "response headers exceeded limit",
                )));
            }

            let n = self.transport.read(&mut tmp).map_err(|_| {
                HttpError::Other(alloc::string::String::from("transport read failed"))
            })?;
            if n == 0 {
                // Transport reported EOF before headers completed. For TLS this
                // is also "no data yet"; TlsStream::read already spins+bounds,
                // so a 0 here from TcpStream means a real FIN — headers can never
                // complete.
                return Err(HttpError::Other(alloc::string::String::from(
                    "connection closed before headers completed",
                )));
            }
            buf.extend_from_slice(&tmp[..n]);
        }
    }

    /// Convenience POST built on [`RequestBuilder`].
    ///
    /// Emits `Content-Type: <content_type>` as the sole extra header; the
    /// builder auto-adds `Host`, `Content-Length`, and `Connection: close`.
    pub fn post(
        &mut self,
        host: &str,
        path: &str,
        content_type: &str,
        body: &[u8],
    ) -> Result<(ParsedHeaders, BodyReader), HttpError> {
        let extra = [("Content-Type", content_type)];
        let req = RequestBuilder::new("POST", path, host, &extra, Some(body)).build();
        self.send(&req)
    }
}
