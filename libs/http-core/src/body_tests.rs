//! Adversarial-fragmentation tests for the [`BodyReader`] decoder.
//!
//! The whole point of isolating Phase 02 in its own host-testable crate is to
//! pound the chunked decoder with byte streams fragmented at every dangerous
//! boundary (mid-size-line, mid-data, between data and its CRLF, ...) and assert
//! the reassembled body is byte-for-byte correct under each pattern.

use super::*;
use crate::response::Framing;

/// A mock transport that hands out a fixed script in caller-chosen fragment
/// sizes.  `fragments` is a queue of how many bytes each successive `read()` is
/// allowed to return; once exhausted it falls back to "as much as fits".  When
/// the script is fully consumed it returns `Ok(0)` forever — modelling a real
/// `embedded_io::Read` transport at EOF (`TcpStream` after FIN).  A complete
/// body reaches `Ok(0)` (completion) before that EOF; a truncated script makes
/// the decoder surface `HttpError::UnexpectedEof`.
struct MockRead {
    data: Vec<u8>,
    pos: usize,
    fragments: Vec<usize>,
    frag_idx: usize,
}

impl MockRead {
    fn new(data: &[u8]) -> Self {
        MockRead {
            data: data.to_vec(),
            pos: 0,
            fragments: Vec::new(),
            frag_idx: 0,
        }
    }

    /// Force each successive read to yield exactly the given counts.
    fn with_fragments(data: &[u8], fragments: &[usize]) -> Self {
        MockRead {
            data: data.to_vec(),
            pos: 0,
            fragments: fragments.to_vec(),
            frag_idx: 0,
        }
    }

    /// One byte per read — the most adversarial split possible.
    fn drip(data: &[u8]) -> Self {
        let n = data.len();
        MockRead::with_fragments(data, &vec![1usize; n])
    }

    fn next_limit(&mut self) -> usize {
        if self.frag_idx < self.fragments.len() {
            let f = self.fragments[self.frag_idx];
            self.frag_idx += 1;
            f
        } else {
            usize::MAX
        }
    }
}

impl embedded_io::ErrorType for MockRead {
    type Error = embedded_io::ErrorKind;
}

impl embedded_io::Read for MockRead {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let remaining = self.data.len().saturating_sub(self.pos);
        let limit = self.next_limit();
        let n = remaining.min(buf.len()).min(limit);
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

/// Drive a reader to completion on a COMPLETE body and return the reassembled
/// bytes.  `out_len` sizes the per-call output buffer to exercise small-buffer
/// multi-read paths.  Any error (including `UnexpectedEof`) is a test failure
/// here — truncation is asserted explicitly by the dedicated EOF tests below.
fn drain_body(reader: &mut BodyReader, t: &mut MockRead, out_len: usize) -> Vec<u8> {
    let mut body = Vec::new();
    let mut out = vec![0u8; out_len];
    let mut guard = 0;
    loop {
        guard += 1;
        assert!(guard < 100_000, "decoder did not terminate");
        match reader.read(t, &mut out) {
            Ok(0) => break,
            Ok(n) => body.extend_from_slice(&out[..n]),
            Err(e) => panic!("unexpected decoder error: {e:?}"),
        }
    }
    body
}

/// Drive a reader expecting a truncated stream: returns the first error the
/// decoder surfaces.  A guard prevents an infinite loop if the FIN-spin bug ever
/// regresses (the decoder must NOT loop forever on a transport at EOF).
fn drain_until_err(reader: &mut BodyReader, t: &mut MockRead, out_len: usize) -> HttpError {
    let mut out = vec![0u8; out_len];
    let mut guard = 0;
    loop {
        guard += 1;
        assert!(
            guard < 100_000,
            "decoder spun without terminating on EOF (FIN-spin regression)"
        );
        match reader.read(t, &mut out) {
            Ok(0) => panic!("expected truncation error, got clean completion"),
            Ok(_) => { /* partial bytes delivered before the EOF surfaces */ }
            Err(e) => return e,
        }
    }
}

// ---------------------------------------------------------------------------
// Content-Length
// ---------------------------------------------------------------------------

#[test]
fn content_length_exact_then_zero() {
    let mut t = MockRead::new(b"Hello, World!");
    let mut r = BodyReader::new(Framing::ContentLength, Some(13), b"");
    let body = drain_body(&mut r, &mut t, 64);
    assert_eq!(body, b"Hello, World!");
    // Subsequent read must report completion.
    let mut out = [0u8; 8];
    assert_eq!(r.read(&mut t, &mut out), Ok(0));
}

#[test]
fn content_length_drip_one_byte_at_a_time() {
    let mut t = MockRead::drip(b"abcdefghij");
    let mut r = BodyReader::new(Framing::ContentLength, Some(10), b"");
    assert_eq!(drain_body(&mut r, &mut t, 4), b"abcdefghij");
}

#[test]
fn content_length_leftover_larger_than_out_buffer() {
    // Entire body arrives in the header packet (leftover); out buffer is small
    // → must take multiple reads to drain leftover.
    let mut t = MockRead::new(b"");
    let mut r = BodyReader::new(Framing::ContentLength, Some(11), b"hello world");
    assert_eq!(drain_body(&mut r, &mut t, 3), b"hello world");
}

#[test]
fn content_length_leftover_plus_transport() {
    let mut t = MockRead::new(b"world");
    let mut r = BodyReader::new(Framing::ContentLength, Some(10), b"hello");
    assert_eq!(drain_body(&mut r, &mut t, 64), b"helloworld");
}

#[test]
fn content_length_zero_is_immediately_complete() {
    let mut t = MockRead::new(b"");
    let mut r = BodyReader::new(Framing::ContentLength, Some(0), b"");
    let mut out = [0u8; 8];
    assert_eq!(r.read(&mut t, &mut out), Ok(0));
}

#[test]
fn content_length_truncated_then_eof_is_unexpected_eof() {
    // Declared 13 bytes; mock delivers 5 then EOFs (Ok(0) forever). The decoder
    // must return UnexpectedEof, NOT spin on NeedMoreData (the FIN-spin bug).
    let mut t = MockRead::new(b"Hello");
    let mut r = BodyReader::new(Framing::ContentLength, Some(13), b"");
    assert_eq!(
        drain_until_err(&mut r, &mut t, 64),
        HttpError::UnexpectedEof
    );
}

#[test]
fn content_length_truncated_leftover_only_then_eof() {
    // 5 bytes arrive in the header packet (leftover), 13 declared, transport
    // empty → after draining leftover the next read EOFs → UnexpectedEof.
    let mut t = MockRead::new(b"");
    let mut r = BodyReader::new(Framing::ContentLength, Some(13), b"Hello");
    assert_eq!(
        drain_until_err(&mut r, &mut t, 64),
        HttpError::UnexpectedEof
    );
}

// Chunked-decoder tests live in their own file but share this module's mock
// transport and `drain_body` harness via `use super::*`.
#[path = "body_chunked_tests.rs"]
mod chunked;
