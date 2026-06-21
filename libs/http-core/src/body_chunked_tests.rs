//! Chunked-decoder tests, split from `body_tests.rs` to keep each file focused.
//! Shares the `MockRead` mock transport and `drain_body` harness from the parent
//! `tests` module (`use super::*`).

use super::*;

// ---------------------------------------------------------------------------
// Happy paths
// ---------------------------------------------------------------------------

const CHUNKED_BODY: &[u8] = b"d\r\nHello, World!\r\n0\r\n\r\n";

#[test]
fn chunked_all_at_once() {
    let mut t = MockRead::new(CHUNKED_BODY);
    let mut r = BodyReader::new(Framing::Chunked, None, b"");
    assert_eq!(drain_body(&mut r, &mut t, 64), b"Hello, World!");
}

#[test]
fn chunked_multiple_chunks() {
    // "Wiki" + "pedia" + " in\r\n\r\nchunks." (RFC example shape).
    let stream = b"4\r\nWiki\r\n5\r\npedia\r\nE\r\n in\r\n\r\nchunks.\r\n0\r\n\r\n";
    let mut t = MockRead::new(stream);
    let mut r = BodyReader::new(Framing::Chunked, None, b"");
    assert_eq!(drain_body(&mut r, &mut t, 64), b"Wikipedia in\r\n\r\nchunks.");
}

#[test]
fn chunked_entire_body_in_leftover() {
    // No transport read should be needed at all.
    let mut t = MockRead::new(b"");
    let mut r = BodyReader::new(Framing::Chunked, None, CHUNKED_BODY);
    assert_eq!(drain_body(&mut r, &mut t, 64), b"Hello, World!");
}

#[test]
fn chunked_first_size_line_in_leftover_data_on_transport() {
    // The header packet carried the size line; the data arrives later.
    let mut t = MockRead::new(b"Hello, World!\r\n0\r\n\r\n");
    let mut r = BodyReader::new(Framing::Chunked, None, b"d\r\n");
    assert_eq!(drain_body(&mut r, &mut t, 64), b"Hello, World!");
}

// ---------------------------------------------------------------------------
// Adversarial fragmentation
// ---------------------------------------------------------------------------

#[test]
fn chunked_drip_one_byte_at_a_time() {
    let mut t = MockRead::drip(CHUNKED_BODY);
    let mut r = BodyReader::new(Framing::Chunked, None, b"");
    assert_eq!(drain_body(&mut r, &mut t, 64), b"Hello, World!");
}

#[test]
fn chunked_size_line_split_across_reads() {
    // "1a" size (26 bytes) arrives as "1" then "a\r\n", then 26 data bytes.
    let payload = b"abcdefghijklmnopqrstuvwxyz"; // 26 bytes == 0x1a
    let mut stream = Vec::new();
    stream.extend_from_slice(b"1a\r\n");
    stream.extend_from_slice(payload);
    stream.extend_from_slice(b"\r\n0\r\n\r\n");
    // Fragments: "1", "a\r\n", then the rest in one go.
    let mut t = MockRead::with_fragments(&stream, &[1, 3]);
    let mut r = BodyReader::new(Framing::Chunked, None, b"");
    assert_eq!(drain_body(&mut r, &mut t, 64), payload);
}

#[test]
fn chunked_data_split_across_reads() {
    let mut t = MockRead::with_fragments(CHUNKED_BODY, &[3, 4, 4]); // splits inside data
    let mut r = BodyReader::new(Framing::Chunked, None, b"");
    assert_eq!(drain_body(&mut r, &mut t, 64), b"Hello, World!");
}

#[test]
fn chunked_data_crlf_boundary_split() {
    // Fragment so that chunk data ends one read and its trailing CRLF the next,
    // and further so \r and \n land in separate reads.
    // Layout: "d\r\n" (3) | "Hello, World!" (13) | "\r" (1) | "\n0\r\n\r\n"
    let mut t = MockRead::with_fragments(CHUNKED_BODY, &[3, 13, 1]);
    let mut r = BodyReader::new(Framing::Chunked, None, b"");
    assert_eq!(drain_body(&mut r, &mut t, 64), b"Hello, World!");
}

#[test]
fn chunked_crlf_fully_split_drip() {
    // Drip guarantees \r and \n of every CRLF arrive in separate reads.
    let stream = b"3\r\nfoo\r\n3\r\nbar\r\n0\r\n\r\n";
    let mut t = MockRead::drip(stream);
    let mut r = BodyReader::new(Framing::Chunked, None, b"");
    assert_eq!(drain_body(&mut r, &mut t, 64), b"foobar");
}

#[test]
fn chunked_small_out_buffer_forces_many_reads() {
    let stream = b"a\r\n0123456789\r\n0\r\n\r\n";
    let mut t = MockRead::new(stream);
    let mut r = BodyReader::new(Framing::Chunked, None, b"");
    // out buffer of 1 byte -> at least 10 reads to drain the chunk.
    assert_eq!(drain_body(&mut r, &mut t, 1), b"0123456789");
}

// ---------------------------------------------------------------------------
// Terminator / trailer variants
// ---------------------------------------------------------------------------

#[test]
fn chunked_terminator_without_trailer() {
    let stream = b"5\r\nhello\r\n0\r\n\r\n";
    let mut t = MockRead::new(stream);
    let mut r = BodyReader::new(Framing::Chunked, None, b"");
    assert_eq!(drain_body(&mut r, &mut t, 64), b"hello");
}

#[test]
fn chunked_terminator_with_trailer_fields() {
    // Trailer: two header-style fields, then the closing CRLF.
    let stream = b"5\r\nhello\r\n0\r\nExpires: Wed\r\nX-Checksum: abc123\r\n\r\n";
    let mut t = MockRead::new(stream);
    let mut r = BodyReader::new(Framing::Chunked, None, b"");
    assert_eq!(drain_body(&mut r, &mut t, 64), b"hello");
    // Fully complete after the trailer.
    let mut out = [0u8; 8];
    assert_eq!(r.read(&mut t, &mut out), Ok(0));
}

#[test]
fn chunked_trailer_split_across_reads() {
    let stream = b"5\r\nhello\r\n0\r\nX-Sig: zzz\r\n\r\n";
    let mut t = MockRead::drip(stream);
    let mut r = BodyReader::new(Framing::Chunked, None, b"");
    assert_eq!(drain_body(&mut r, &mut t, 64), b"hello");
}

#[test]
fn chunked_size_with_extension_is_ignored() {
    // chunk-ext after the size must be skipped by parse_chunk_size.
    let stream = b"5;name=value\r\nhello\r\n0\r\n\r\n";
    let mut t = MockRead::new(stream);
    let mut r = BodyReader::new(Framing::Chunked, None, b"");
    assert_eq!(drain_body(&mut r, &mut t, 64), b"hello");
}

// ---------------------------------------------------------------------------
// Malformed / hostile
// ---------------------------------------------------------------------------

#[test]
fn chunked_malformed_size_is_bad_chunk() {
    // 'g' is not a hex digit and not a valid ext/terminator byte.
    let stream = b"g\r\nhello\r\n0\r\n\r\n";
    let mut t = MockRead::new(stream);
    let mut r = BodyReader::new(Framing::Chunked, None, b"");
    let mut out = [0u8; 64];
    assert_eq!(r.read(&mut t, &mut out), Err(HttpError::BadChunk));
}

#[test]
fn chunked_size_over_ceiling_is_bad_chunk() {
    // 0x2000000 == 32 MiB > MAX_CHUNK_SIZE (16 MiB).
    let stream = b"2000000\r\n";
    let mut t = MockRead::new(stream);
    let mut r = BodyReader::new(Framing::Chunked, None, b"");
    let mut out = [0u8; 64];
    assert_eq!(r.read(&mut t, &mut out), Err(HttpError::BadChunk));
}

#[test]
fn chunked_oversized_size_line_is_bad_chunk() {
    // A size "line" that never terminates within LINE_BUF_LEN bytes.
    let mut data = Vec::new();
    data.extend_from_slice(&[b'0'; 64]); // 64 hex zeros, no CRLF in 32 bytes
    let mut t = MockRead::new(&data);
    let mut r = BodyReader::new(Framing::Chunked, None, b"");
    let mut out = [0u8; 64];
    assert_eq!(r.read(&mut t, &mut out), Err(HttpError::BadChunk));
}

// ---------------------------------------------------------------------------
// Truncation (transport FIN mid-stream) -> UnexpectedEof, never a spin
// ---------------------------------------------------------------------------

#[test]
fn chunked_eof_mid_chunk_data_is_unexpected_eof() {
    // Chunk declares 0xd (13) data bytes but only 5 arrive before EOF.
    let stream = b"d\r\nHello";
    let mut t = MockRead::new(stream);
    let mut r = BodyReader::new(Framing::Chunked, None, b"");
    assert_eq!(drain_until_err(&mut r, &mut t, 64), HttpError::UnexpectedEof);
}

#[test]
fn chunked_eof_mid_size_line_is_unexpected_eof() {
    // Size line "1a" never terminates with CRLF before EOF.
    let stream = b"1a";
    let mut t = MockRead::new(stream);
    let mut r = BodyReader::new(Framing::Chunked, None, b"");
    assert_eq!(drain_until_err(&mut r, &mut t, 64), HttpError::UnexpectedEof);
}

#[test]
fn chunked_eof_before_terminator_is_unexpected_eof() {
    // A complete data chunk + its CRLF, then EOF before the `0\r\n` terminator.
    let stream = b"5\r\nhello\r\n";
    let mut t = MockRead::new(stream);
    let mut r = BodyReader::new(Framing::Chunked, None, b"");
    assert_eq!(drain_until_err(&mut r, &mut t, 64), HttpError::UnexpectedEof);
}

#[test]
fn chunked_eof_mid_trailer_is_unexpected_eof() {
    // `0` terminator seen, a trailer field begins, then EOF before final CRLF.
    let stream = b"5\r\nhello\r\n0\r\nX-Sig: zz";
    let mut t = MockRead::new(stream);
    let mut r = BodyReader::new(Framing::Chunked, None, b"");
    assert_eq!(drain_until_err(&mut r, &mut t, 64), HttpError::UnexpectedEof);
}

#[test]
fn chunked_bad_trailing_crlf_is_bad_chunk() {
    // Chunk data not followed by CRLF — corrupt framing.
    let stream = b"5\r\nhelloXX0\r\n\r\n";
    let mut t = MockRead::new(stream);
    let mut r = BodyReader::new(Framing::Chunked, None, b"");
    let mut out = [0u8; 64];
    let mut saw_bad = false;
    for _ in 0..10 {
        match r.read(&mut t, &mut out) {
            Ok(0) => break,
            Ok(_) => {}
            Err(HttpError::BadChunk) => {
                saw_bad = true;
                break;
            }
            Err(e) => panic!("unexpected: {e:?}"),
        }
    }
    assert!(saw_bad, "expected BadChunk on corrupt trailing CRLF");
}
