//! HTTP/1.1 response header parsing.
//!
//! Wraps `httparse::Response::parse` with a fixed-size header array
//! (capped at `MAX_HEADERS = 32`) and maps the result to `ParsedHeaders`.
//! Deliberate omission: `UntilClose` framing is not supported (plan decision 4)
//! because TLS transports cannot distinguish "no data yet" from "connection
//! closed" reliably; callers must rely on `Content-Length` or chunked framing.

use crate::MAX_HEADERS;
use alloc::string::String;

/// How the response body is delimited.
///
/// `UntilClose` is intentionally absent — see module doc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framing {
    /// Body ends after exactly `content_length` bytes.
    ContentLength,
    /// Body uses chunked transfer encoding (`Transfer-Encoding: chunked`).
    Chunked,
}

/// Structured result of a successful header parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedHeaders {
    /// HTTP status code, e.g. `200`.
    pub status: u16,
    /// Value of the `Content-Length` header, if present and valid UTF-8 decimal.
    pub content_length: Option<usize>,
    /// Body framing strategy derived from response headers.
    pub framing: Framing,
    /// Byte offset within the input buffer where the body starts
    /// (i.e. the position just after `\r\n\r\n`).
    pub body_offset: usize,
}

/// Errors produced while parsing a response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpError {
    /// The header buffer is a valid prefix but more bytes are needed to finish
    /// the header block. Emitted ONLY by [`parse_response_headers`] — the caller
    /// should accumulate more bytes and retry. The body decoder never returns
    /// this: a transport 0-read mid-body is [`UnexpectedEof`], not "retry".
    NeedMoreData,
    /// The transport returned EOF (`Ok(0)`) while the framed body was still
    /// incomplete — the connection closed before all declared/chunked bytes
    /// arrived (a truncated response). Per `embedded_io::Read`, `Ok(0)` means
    /// "no more bytes will ever come": `TcpStream::read` returns it only on FIN,
    /// and `TlsStream::read` converts its no-progress budget to `Err` rather than
    /// `Ok(0)`. So this is definitive truncation, never "try again".
    UnexpectedEof,
    /// The status line is syntactically invalid.
    MalformedStatus,
    /// A header line is syntactically invalid.
    MalformedHeader,
    /// The server sent more than [`MAX_HEADERS`] headers.
    TooManyHeaders,
    /// A chunked body contains an invalid chunk-size line.
    BadChunk,
    /// Catch-all for other protocol errors.
    Other(String),
}

/// Parse the headers from a raw HTTP/1.1 response buffer.
///
/// # Precondition
///
/// `buf` must contain the complete headers (ending in `\r\n\r\n`) plus any
/// leading body bytes that have already arrived.  If the headers are
/// incomplete, returns `HttpError::NeedMoreData` — the caller should accumulate
/// more bytes and retry.
///
/// # Header cap
///
/// Responses with more than [`MAX_HEADERS`] (32) headers are rejected with
/// `HttpError::TooManyHeaders`.  This bounds allocation without a heap-based
/// dynamic vector and guards against hostile servers.
///
/// # Framing priority
///
/// If `Transfer-Encoding: chunked` is present it takes precedence over any
/// `Content-Length` header (RFC 9112 §6.1).
///
/// # Unframed responses (no Content-Length, no chunked)
///
/// A response with neither header yields `Framing::ContentLength` with
/// `content_length: None`, which [`BodyReader`][crate::BodyReader] treats as a
/// **zero-length body**.  This is correct for statuses that conventionally carry
/// no body (`204 No Content`, `304 Not Modified`).  A body-bearing unframed
/// response would need `UntilClose` framing, which is deliberately unsupported
/// (plan decision 4 — see this module's doc): such a body is silently treated as
/// empty rather than read until the connection closes.
pub fn parse_response_headers(buf: &[u8]) -> Result<ParsedHeaders, HttpError> {
    // Fixed-size header array on the stack — no dynamic allocation for parsing.
    let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let mut response = httparse::Response::new(&mut headers);

    let n = match response.parse(buf) {
        Ok(httparse::Status::Complete(n)) => n,
        Ok(httparse::Status::Partial) => return Err(HttpError::NeedMoreData),
        Err(httparse::Error::TooManyHeaders) => return Err(HttpError::TooManyHeaders),
        Err(httparse::Error::Status) => return Err(HttpError::MalformedStatus),
        Err(_) => return Err(HttpError::MalformedHeader),
    };

    let status = response.code.ok_or(HttpError::MalformedStatus)?;

    // Walk headers once: check Transfer-Encoding, collect Content-Length.
    let mut content_length: Option<usize> = None;
    let mut is_chunked = false;

    for hdr in response.headers.iter() {
        // httparse returns empty-name sentinels for unused slots; skip them.
        if hdr.name.is_empty() {
            break;
        }
        // Case-insensitive header name comparison (ASCII; RFC 9110 §5.1)
        if hdr.name.eq_ignore_ascii_case("transfer-encoding") {
            // The value may be comma-separated; "chunked" is the interesting token.
            if let Ok(val) = core::str::from_utf8(hdr.value) {
                if val
                    .split(',')
                    .any(|t| t.trim().eq_ignore_ascii_case("chunked"))
                {
                    is_chunked = true;
                }
            }
        } else if hdr.name.eq_ignore_ascii_case("content-length") {
            if let Ok(val) = core::str::from_utf8(hdr.value) {
                // Silently ignore non-parseable Content-Length (keep None).
                if let Ok(n) = val.trim().parse::<usize>() {
                    content_length = Some(n);
                }
            }
        }
    }

    // Transfer-Encoding: chunked overrides Content-Length (RFC 9112 §6.1).
    let framing = if is_chunked {
        Framing::Chunked
    } else {
        Framing::ContentLength
    };

    Ok(ParsedHeaders {
        status,
        content_length,
        framing,
        body_offset: n,
    })
}

#[cfg(test)]
#[path = "response_tests.rs"]
mod tests;
