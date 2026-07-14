//! Byte-source helpers shared by the chunked decoder: pull bytes from the
//! seeded `leftover` region first, then from the transport.  A 0-length
//! transport read while the body is incomplete is a genuine EOF (the connection
//! closed mid-body), surfaced as [`Byte::Eof`] — the callers translate that into
//! `HttpError::UnexpectedEof` (see the `body` module doc).
//!
//! CRITICAL distinction: [`Byte::Eof`] / `fill() == 0` are returned ONLY when a
//! real `transport.read()` produced 0 bytes.  Leftover bytes are always served
//! first, so "leftover drained but transport not yet polled" can never be
//! mistaken for EOF here — the transport is polled in the same call.

use super::{BodyReader, HttpError, Read};

/// Outcome of pulling a single byte from the leftover/transport source.
pub(super) enum Byte {
    Got(u8),
    /// Transport returned a 0-length read (EOF) with no leftover available.
    Eof,
}

impl BodyReader {
    /// Pull a single byte from leftover-then-transport.
    pub(super) fn next_byte<R: Read>(&mut self, transport: &mut R) -> Result<Byte, HttpError> {
        if !self.leftover_empty() {
            let b = self.leftover[self.leftover_pos];
            self.leftover_pos += 1;
            return Ok(Byte::Got(b));
        }
        let mut one = [0u8; 1];
        let n = transport
            .read(&mut one)
            .map_err(|e| HttpError::Other(alloc::format!("transport read: {e:?}")))?;
        if n == 0 {
            Ok(Byte::Eof)
        } else {
            Ok(Byte::Got(one[0]))
        }
    }

    /// Fill `dst` from leftover-then-transport, returning bytes copied.  Returns
    /// `0` only when the transport itself returned 0 (EOF) with no leftover
    /// available — never for an empty `dst` quirk (callers guard `dst`).
    pub(super) fn fill<R: Read>(
        &mut self,
        transport: &mut R,
        dst: &mut [u8],
    ) -> Result<usize, HttpError> {
        if dst.is_empty() {
            return Ok(0);
        }
        if !self.leftover_empty() {
            return Ok(self.drain_leftover(dst));
        }
        transport
            .read(dst)
            .map_err(|e| HttpError::Other(alloc::format!("transport read: {e:?}")))
    }
}
