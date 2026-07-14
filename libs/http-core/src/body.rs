//! Incremental HTTP/1.1 response body decoder (`BodyReader`).
//!
//! Decodes **Content-Length** and **chunked** framing over a blocking
//! `embedded_io::Read` transport, returning body bytes a slice at a time
//! WITHOUT ever buffering the whole (possibly unbounded, e.g. a streaming LLM
//! response) body.  `UntilClose` framing is deliberately unsupported (plan
//! decision 4): a TLS transport cannot distinguish "no data yet" from
//! "connection closed", so there is no reliable EOF signal to terminate on.
//!
//! # Completion vs. transport EOF (the load-bearing contract)
//!
//! `read()` returning `Ok(0)` means the body is **complete per its framing** —
//! Content-Length reached `remaining == 0`, or the chunked stream hit its
//! `0\r\n` terminator.  That is the ONLY success-completion signal.
//!
//! When the underlying `transport.read()` returns `0` while the body is still
//! **incomplete**, that is a genuine EOF: per `embedded_io::Read`, `Ok(0)` means
//! "no more bytes will ever come".  Both real transports honour this —
//! `TcpStream::read` returns `0` only on FIN (it loops internally on a transient
//! empty read), and `TlsStream::read` never returns `0` (it converts a
//! no-progress retry budget to `Err`).  So a 0-read mid-body is a definitive
//! truncation, and `BodyReader` returns `Err(HttpError::UnexpectedEof)` — NOT
//! `NeedMoreData`.  Returning `NeedMoreData` here would make a `NeedMoreData →
//! retry` caller spin forever over a closed TCP connection (the FIN-spin bug).
//!
//! `HttpError::NeedMoreData` belongs exclusively to header parsing
//! (`parse_response_headers`); the body decoder never emits it.  Keeping all
//! transport policy (retry, timeout) out of `http-core` is what lets this crate
//! stay pure and host-testable: no spin, no yield, no timer lives here.

use crate::response::{Framing, HttpError};
use alloc::vec::Vec;
use embedded_io::Read;

/// Size of the chunk-size line accumulator.
///
/// A chunk-size line is `1*HEXDIG [ chunk-ext ] CRLF`.  httparse rejects more
/// than 16 hex digits, so 16 + CRLF = 18 bytes covers any well-formed size with
/// no extension; 32 leaves room for a short extension token while still bounding
/// internal state.  A size line that does not complete within 32 bytes is
/// treated as malformed (`HttpError::BadChunk`) — defends against a hostile
/// server streaming an unbounded "size line".
const LINE_BUF_LEN: usize = 32;

/// Upper bound on a single chunk's declared size, guarding the `u64 -> usize`
/// cast.  16 MiB comfortably exceeds any realistic response chunk yet stays far
/// below `usize::MAX` even on 32-bit targets (riscv32: `usize` is 4 bytes), so
/// the cast below can never truncate.  A chunk claiming more is rejected as
/// `HttpError::BadChunk` rather than risking a wraparound.
const MAX_CHUNK_SIZE: u64 = 16 * 1024 * 1024;

/// Which part of a chunk the decoder expects next.
///
/// NOTE (spec deviation): the phase spec listed the chunked state as
/// `{ chunk_remaining, line_buf, line_len, done }`.  An explicit `phase` is
/// added so the "data fully delivered, trailing CRLF not yet consumed" moment
/// is representable WITHOUT eagerly consuming the CRLF in the same call that
/// returned the data — eager consumption would lose returned bytes if the CRLF
/// were split across reads.  This is a correctness fix, documented per the
/// no-silent-deviation rule.
#[derive(PartialEq, Eq, Clone, Copy)]
enum ChunkPhase {
    /// Expecting (the next byte of) a chunk-size line.
    Size,
    /// Inside a chunk's data section (`chunk_remaining` bytes still owed).
    Data,
    /// Data delivered; expecting the 2-byte CRLF that trails it.
    TrailingCrlf,
    /// `0`-size terminator seen; draining the trailer up to its final CRLF.
    Trailer,
}

/// Decoder state, one variant per supported framing.
enum State {
    /// Content-Length framing: serve bytes until `remaining` hits zero.
    ContentLength { remaining: usize },
    /// Chunked framing: drives size-line -> data -> trailing-CRLF -> repeat.
    Chunked {
        /// What the decoder expects to read next.
        phase: ChunkPhase,
        /// Bytes still owed from the current chunk's data section.
        chunk_remaining: u64,
        /// Accumulator for a chunk-size line split across transport reads.
        line_buf: [u8; LINE_BUF_LEN],
        /// Valid prefix length within `line_buf`.
        line_len: usize,
        /// Set once the `0\r\n` terminator (and its trailer) is fully consumed.
        done: bool,
    },
}

/// Incremental body reader.  Construct with [`BodyReader::new`], then call
/// [`BodyReader::read`] repeatedly until it returns `Ok(0)`.
pub struct BodyReader {
    state: State,
    /// Bytes already received past the header terminator during header parsing.
    /// CRITICAL: the first chunk-size line frequently arrives in the SAME packet
    /// as the headers, so these MUST be consumed before any transport read.
    leftover: Vec<u8>,
    /// Read cursor into `leftover`.
    leftover_pos: usize,
}

impl BodyReader {
    /// Build a reader for `framing`.
    ///
    /// `content_length` is required (and used) only for `Framing::ContentLength`;
    /// it is ignored for chunked.  A `ContentLength` framing with `None` length
    /// is treated as a zero-length body (the header parser only yields
    /// `ContentLength` when it saw a valid length, but we stay total here).
    ///
    /// `leftover` is the slice of post-header bytes already buffered by the
    /// caller; it is copied in and drained before the transport is touched.
    pub fn new(framing: Framing, content_length: Option<usize>, leftover: &[u8]) -> Self {
        let state = match framing {
            Framing::ContentLength => State::ContentLength {
                remaining: content_length.unwrap_or(0),
            },
            Framing::Chunked => State::Chunked {
                phase: ChunkPhase::Size,
                chunk_remaining: 0,
                line_buf: [0u8; LINE_BUF_LEN],
                line_len: 0,
                done: false,
            },
        };
        BodyReader {
            state,
            leftover: leftover.to_vec(),
            leftover_pos: 0,
        }
    }

    /// Decode the next slice of body into `out`.
    ///
    /// Returns the number of bytes written (`<= out.len()`).  `Ok(0)` means the
    /// body is complete per its framing.  A transport 0-read on an incomplete
    /// body yields `Err(HttpError::UnexpectedEof)` (truncated).  See the module
    /// doc for the precise completion-vs-EOF contract.
    pub fn read<R: Read>(&mut self, transport: &mut R, out: &mut [u8]) -> Result<usize, HttpError> {
        if out.is_empty() {
            return Ok(0);
        }
        match &mut self.state {
            State::ContentLength { .. } => self.read_content_length(transport, out),
            State::Chunked { .. } => self.read_chunked(transport, out),
        }
    }

    /// Drain pending leftover bytes into `dst`, returning how many were copied.
    fn drain_leftover(&mut self, dst: &mut [u8]) -> usize {
        let avail = &self.leftover[self.leftover_pos..];
        let n = avail.len().min(dst.len());
        dst[..n].copy_from_slice(&avail[..n]);
        self.leftover_pos += n;
        n
    }

    /// True once all buffered leftover bytes have been consumed.
    fn leftover_empty(&self) -> bool {
        self.leftover_pos >= self.leftover.len()
    }
}

#[path = "body_chunked.rs"]
mod chunked;
#[path = "body_content_length.rs"]
mod content_length;
#[path = "body_source.rs"]
mod source;

#[cfg(test)]
#[path = "body_tests.rs"]
mod tests;
