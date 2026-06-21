//! Content-Length read path for [`BodyReader`].
//!
//! Serves bytes from the seeded `leftover` first, then from the transport,
//! counting down `remaining`.  Returns `Ok(0)` exactly when the declared
//! Content-Length has been fully delivered.  A transport 0-read BEFORE
//! `remaining` hits zero is a truncated response (`UnexpectedEof`), not "retry".

use super::{BodyReader, HttpError, Read, State};

impl BodyReader {
    pub(super) fn read_content_length<R: Read>(
        &mut self,
        transport: &mut R,
        out: &mut [u8],
    ) -> Result<usize, HttpError> {
        let remaining = match &self.state {
            State::ContentLength { remaining } => *remaining,
            // Unreachable: dispatched only for the ContentLength variant.
            _ => return Err(HttpError::BadChunk),
        };
        if remaining == 0 {
            return Ok(0);
        }

        // Cap this call to whatever is still owed and what fits in `out`.
        let want = remaining.min(out.len());

        // Leftover (header-packet) bytes take priority over a transport read.
        if !self.leftover_empty() {
            let n = self.drain_leftover(&mut out[..want]);
            self.decrement_content_length(n);
            return Ok(n);
        }

        // No buffered bytes left — pull from the transport.
        let n = transport
            .read(&mut out[..want])
            .map_err(|e| HttpError::Other(alloc::format!("transport read: {e:?}")))?;
        if n == 0 {
            // Transport hit EOF (`Ok(0)`) with `remaining > 0`: the connection
            // closed before the declared Content-Length was delivered — a
            // truncated response.  Per the transport contract a 0-read is
            // definitive (no more bytes will come), so this is a hard error, not
            // a "retry" signal (see module doc).
            return Err(HttpError::UnexpectedEof);
        }
        self.decrement_content_length(n);
        Ok(n)
    }

    fn decrement_content_length(&mut self, n: usize) {
        if let State::ContentLength { remaining } = &mut self.state {
            *remaining = remaining.saturating_sub(n);
        }
    }
}
