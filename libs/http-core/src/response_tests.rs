use super::*;

const OK_CONTENT_LENGTH: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 13\r\n\r\nHello, World!";

const OK_CHUNKED: &[u8] =
    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nd\r\nHello, World!\r\n0\r\n\r\n";

#[test]
fn parse_200_content_length() {
    let h = parse_response_headers(OK_CONTENT_LENGTH).unwrap();
    assert_eq!(h.status, 200);
    assert_eq!(h.framing, Framing::ContentLength);
    assert_eq!(h.content_length, Some(13));
    assert_eq!(&OK_CONTENT_LENGTH[h.body_offset..], b"Hello, World!");
}

#[test]
fn parse_200_chunked_framing() {
    let h = parse_response_headers(OK_CHUNKED).unwrap();
    assert_eq!(h.status, 200);
    assert_eq!(h.framing, Framing::Chunked);
    assert_eq!(h.content_length, None);
}

#[test]
fn truncated_headers_need_more_data() {
    let partial = b"HTTP/1.1 200 OK\r\nContent-Type: text/";
    assert_eq!(
        parse_response_headers(partial),
        Err(HttpError::NeedMoreData)
    );
}

#[test]
fn empty_buffer_need_more_data() {
    assert_eq!(parse_response_headers(b""), Err(HttpError::NeedMoreData));
}

#[test]
fn malformed_status_line() {
    let bad = b"NOTHTTP\r\n\r\n";
    let err = parse_response_headers(bad);
    assert!(
        matches!(
            err,
            Err(HttpError::MalformedStatus) | Err(HttpError::MalformedHeader)
        ),
        "expected malformed error, got {err:?}"
    );
}

#[test]
fn non_200_status_code() {
    let resp = b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
    let h = parse_response_headers(resp).unwrap();
    assert_eq!(h.status, 404);
}

#[test]
fn chunked_overrides_content_length() {
    // RFC 9112 §6.1: Transfer-Encoding wins when both headers are present.
    let resp = b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nTransfer-Encoding: chunked\r\n\r\n";
    let h = parse_response_headers(resp).unwrap();
    assert_eq!(h.framing, Framing::Chunked);
    assert_eq!(h.content_length, Some(100));
}

#[test]
fn body_offset_correct() {
    let resp = b"HTTP/1.1 200 OK\r\nX-Foo: bar\r\n\r\nbody data";
    let h = parse_response_headers(resp).unwrap();
    assert_eq!(&resp[h.body_offset..], b"body data");
}

#[test]
fn no_body_bytes_yet() {
    // Headers complete but body not arrived — body_offset points to end of buf.
    let resp = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\n";
    let h = parse_response_headers(resp).unwrap();
    assert_eq!(h.body_offset, resp.len());
}
