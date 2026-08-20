use super::*;

#[test]
fn parses_client_sequence_and_payload() {
    let mut buf = [0u8; IPC_BUF_SIZE];
    buf[..8].copy_from_slice(&9u64.to_le_bytes());
    buf[8..10].copy_from_slice(&3u16.to_le_bytes());
    buf[10..13].copy_from_slice(b"abc");
    let parsed = parse_request(&buf).expect("request parses");
    assert_eq!(parsed.client_sequence, 9);
    assert_eq!(parsed.payload_len, 3);
    assert_eq!(&parsed.payload[..3], b"abc");
}

#[test]
fn rejects_payloads_beyond_local_cap() {
    let mut buf = [0u8; IPC_BUF_SIZE];
    buf[8..10].copy_from_slice(&((MAX_REQUEST_BODY + 1) as u16).to_le_bytes());
    assert_eq!(parse_request(&buf), Err(ParseError::PayloadTooLarge));
}

#[test]
fn rejects_short_buffers_without_panicking() {
    let buf = [0u8; 9];
    assert_eq!(parse_request(&buf), Err(ParseError::ShortBuffer));
}

#[test]
fn rejects_truncated_payloads_without_panicking() {
    let mut buf = [0u8; IPC_BUF_SIZE];
    buf[..8].copy_from_slice(&9u64.to_le_bytes());
    buf[8..10].copy_from_slice(&4u16.to_le_bytes());
    assert_eq!(parse_request(&buf[..12]), Err(ParseError::TruncatedPayload));
}

#[test]
fn encodes_status_request_id_and_sequence() {
    let mut out = [0u8; IPC_BUF_SIZE];
    let len = encode_reply(ReplyStatus::Busy, 7, 11, b"xy", &mut out);
    assert_eq!(len, RESPONSE_HEADER_LEN + 2);
    assert_eq!(&out[..2], &[STATUS_TAG, ReplyStatus::Busy as u8]);
    assert_eq!(u64::from_le_bytes(out[2..10].try_into().unwrap()), 7);
    assert_eq!(u64::from_le_bytes(out[10..18].try_into().unwrap()), 11);
    assert_eq!(u16::from_le_bytes(out[18..20].try_into().unwrap()), 2);
    assert_eq!(&out[20..22], b"xy");
}
