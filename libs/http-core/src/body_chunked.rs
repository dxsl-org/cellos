//! Chunked transfer-encoding decoder for [`BodyReader`].
//!
//! Drives the classic chunked state machine via [`ChunkPhase`]:
//!
//! ```text
//!   Size  ->  Data  ->  TrailingCrlf  ->  Size  ->  ...
//!   ... a "0" size line switches to Trailer, drained to its final CRLF, done.
//! ```
//!
//! Every byte is sourced through [`fill`] / [`next_byte`], which drain the
//! seeded `leftover` (the bytes that arrived in the same packet as the headers)
//! before touching the transport.  A 0-length transport read while the body is
//! incomplete is a genuine EOF — the connection closed mid-body, so it is
//! surfaced as `HttpError::UnexpectedEof` (a truncated chunked stream), never as
//! a "retry" signal (see module doc).
//!
//! Three classic bugs this defends against (research finding 5):
//!  1. not consuming the CRLF that trails each chunk's data (the `TrailingCrlf`
//!     phase is a distinct, resumable step);
//!  2. a chunk-size line split across two transport reads (accumulated in
//!     `line_buf`, retried via `parse_chunk_size`'s `Partial`);
//!  3. `u64 -> usize` overflow on a hostile chunk size (capped at
//!     `MAX_CHUNK_SIZE`).

use super::source::Byte;
use super::{
    BodyReader, ChunkPhase, HttpError, Read, State, LINE_BUF_LEN, MAX_CHUNK_SIZE,
};

impl BodyReader {
    pub(super) fn read_chunked<R: Read>(
        &mut self,
        transport: &mut R,
        out: &mut [u8],
    ) -> Result<usize, HttpError> {
        loop {
            let phase = match &self.state {
                State::Chunked { done: true, .. } => return Ok(0),
                State::Chunked { phase, .. } => *phase,
                _ => return Err(HttpError::BadChunk),
            };
            match phase {
                ChunkPhase::Size => {
                    // Parse the next size line; loop on to serve data, or finish.
                    match self.parse_size_line(transport)? {
                        // EOF mid-size-line: stream truncated before a complete
                        // chunk-size header arrived.
                        None => return Err(HttpError::UnexpectedEof),
                        Some(0) => self.set_phase(ChunkPhase::Trailer),
                        Some(size) => {
                            self.begin_chunk(size);
                            // fall through the loop into the Data phase
                        }
                    }
                }
                ChunkPhase::Data => return self.serve_chunk_data(transport, out),
                ChunkPhase::TrailingCrlf => {
                    // Resumable: consume the 2-byte CRLF, then back to Size.
                    self.consume_crlf(transport)?;
                    self.set_phase(ChunkPhase::Size);
                }
                ChunkPhase::Trailer => {
                    self.drain_trailer(transport)?;
                    if let State::Chunked { done, .. } = &mut self.state {
                        *done = true;
                    }
                    return Ok(0);
                }
            }
        }
    }

    /// Copy chunk data into `out`.  When the chunk is fully delivered, advance to
    /// `TrailingCrlf` (the CRLF is consumed on the NEXT call, so already-returned
    /// data is never lost to a split-CRLF error).
    fn serve_chunk_data<R: Read>(
        &mut self,
        transport: &mut R,
        out: &mut [u8],
    ) -> Result<usize, HttpError> {
        let remaining = match &self.state {
            State::Chunked { chunk_remaining, .. } => *chunk_remaining,
            _ => return Err(HttpError::BadChunk),
        };
        // `remaining` fits in usize: bounded by MAX_CHUNK_SIZE at parse time.
        let want = (remaining as usize).min(out.len());
        let mut written = 0;
        while written < want {
            match self.fill(transport, &mut out[written..want])? {
                0 => {
                    // Transport hit EOF mid-chunk: the chunk's `chunk_remaining`
                    // data bytes never fully arrived → truncated stream.  (If we
                    // already copied some bytes this call, return them first; the
                    // EOF surfaces on the next call when `written == 0`.)
                    if written == 0 {
                        return Err(HttpError::UnexpectedEof);
                    }
                    break;
                }
                n => written += n,
            }
        }

        let new_remaining = remaining - written as u64;
        if let State::Chunked { chunk_remaining, phase, .. } = &mut self.state {
            *chunk_remaining = new_remaining;
            if new_remaining == 0 {
                *phase = ChunkPhase::TrailingCrlf;
            }
        }
        Ok(written)
    }

    fn begin_chunk(&mut self, size: u64) {
        if let State::Chunked { chunk_remaining, phase, line_len, .. } = &mut self.state {
            *chunk_remaining = size;
            *line_len = 0;
            *phase = ChunkPhase::Data;
        }
    }

    fn set_phase(&mut self, p: ChunkPhase) {
        if let State::Chunked { phase, .. } = &mut self.state {
            *phase = p;
        }
    }

    /// Accumulate and parse a chunk-size line.  Returns `Some(size)` when the
    /// line is complete, `None` if more transport data is needed (caller retry).
    fn parse_size_line<R: Read>(
        &mut self,
        transport: &mut R,
    ) -> Result<Option<u64>, HttpError> {
        loop {
            let line_len = match &self.state {
                State::Chunked { line_len, .. } => *line_len,
                _ => return Err(HttpError::BadChunk),
            };
            if line_len > 0 {
                let parsed = match &self.state {
                    State::Chunked { line_buf, .. } => {
                        httparse::parse_chunk_size(&line_buf[..line_len])
                    }
                    _ => return Err(HttpError::BadChunk),
                };
                match parsed {
                    Ok(httparse::Status::Complete((consumed, size))) => {
                        if size > MAX_CHUNK_SIZE {
                            return Err(HttpError::BadChunk);
                        }
                        self.requeue_after_size_line(consumed, line_len);
                        return Ok(Some(size));
                    }
                    Ok(httparse::Status::Partial) => { /* need another byte */ }
                    Err(_) => return Err(HttpError::BadChunk),
                }
            }
            match self.next_byte(transport)? {
                // EOF before the size line terminated: caller maps None to
                // UnexpectedEof (truncated mid-size-line).
                Byte::Eof => return Ok(None),
                Byte::Got(b) => self.push_line_byte(b)?,
            }
        }
    }

    /// Push one byte into the size-line accumulator, guarding its capacity.
    fn push_line_byte(&mut self, b: u8) -> Result<(), HttpError> {
        if let State::Chunked { line_buf, line_len, .. } = &mut self.state {
            if *line_len >= LINE_BUF_LEN {
                // A size line longer than the buffer is malformed/hostile.
                return Err(HttpError::BadChunk);
            }
            line_buf[*line_len] = b;
            *line_len += 1;
            Ok(())
        } else {
            Err(HttpError::BadChunk)
        }
    }

    /// Reset the size-line accumulator after a size line is fully parsed.
    ///
    /// INVARIANT: `consumed == line_len`.  `parse_size_line` feeds exactly one
    /// byte into `line_buf` and re-parses after each, so `parse_chunk_size`
    /// signals `Complete` on the very byte that finishes the trailing CRLF —
    /// there are never leftover data bytes in `line_buf` to splice back.  We keep
    /// the `line_len` argument only to assert that invariant.
    fn requeue_after_size_line(&mut self, consumed: usize, line_len: usize) {
        debug_assert_eq!(
            consumed, line_len,
            "size-line accumulator parsed one byte at a time; no trailing data expected",
        );
        if let State::Chunked { line_len, .. } = &mut self.state {
            *line_len = 0;
        }
    }

    /// Consume an expected CRLF.  Tolerates the split where `\r` and `\n` arrive
    /// in separate reads by sourcing each byte through `next_byte`.
    fn consume_crlf<R: Read>(&mut self, transport: &mut R) -> Result<(), HttpError> {
        for &expected in b"\r\n" {
            match self.next_byte(transport)? {
                // EOF mid-CRLF: the chunk's trailing CRLF never arrived.
                Byte::Eof => return Err(HttpError::UnexpectedEof),
                Byte::Got(b) if b == expected => {}
                Byte::Got(_) => return Err(HttpError::BadChunk),
            }
        }
        Ok(())
    }

    /// Drain the trailer after the `0` terminator up to its final empty line.
    /// A trailer is `*(trailer-field CRLF) CRLF`; everything is discarded.  The
    /// "0" line's own CRLF was already consumed by `parse_chunk_size`, so the
    /// first thing seen here is either the closing CRLF (no trailer fields) or a
    /// trailer field.
    fn drain_trailer<R: Read>(&mut self, transport: &mut R) -> Result<(), HttpError> {
        let mut prev_cr = false;
        let mut line_has_content = false;
        loop {
            match self.next_byte(transport)? {
                // EOF before the trailer's final blank line: truncated stream.
                Byte::Eof => return Err(HttpError::UnexpectedEof),
                Byte::Got(b'\r') => {
                    // A `\r` after a `\r` is itself trailer content (a malformed
                    // `\r\r` line), so the prior `\r` did not start a CRLF.
                    if prev_cr {
                        line_has_content = true;
                    }
                    prev_cr = true;
                }
                Byte::Got(b'\n') if prev_cr => {
                    if !line_has_content {
                        return Ok(()); // blank line -> end of trailer
                    }
                    prev_cr = false;
                    line_has_content = false;
                }
                Byte::Got(_) => {
                    prev_cr = false;
                    line_has_content = true;
                }
            }
        }
    }
}
