use super::*;

#[test]
fn parse_close_exact() {
    let mut buf = [0u8; 9];
    buf[0] = TLS_CLOSE_OP;
    buf[1..9].copy_from_slice(&42u64.to_le_bytes());
    assert_eq!(parse_raw_tls_request(&buf), Ok(RawTlsRequest::Close { cap: 42 }));
}

#[test]
fn parse_connect_exact_and_negative() {
    // Valid
    let mut buf = alloc::vec![0u8; 17 + 11];
    buf[0] = TLS_CONNECT_OP;
    buf[9..13].copy_from_slice(&[10, 0, 2, 2]);
    buf[13..15].copy_from_slice(&443u16.to_le_bytes());
    buf[15..17].copy_from_slice(&11u16.to_le_bytes());
    buf[17..17 + 11].copy_from_slice(b"example.com");
    assert_eq!(
        parse_raw_tls_request(&buf),
        Ok(RawTlsRequest::Connect {
            addr: [10, 0, 2, 2],
            port: 443,
            hostname: "example.com",
        })
    );

    // Truncated buffer
    assert_eq!(parse_raw_tls_request(&buf[..16]), Err(RawTlsError::BufferTooShort));

    // Declared length exceeding buffer
    buf[15..17].copy_from_slice(&50u16.to_le_bytes());
    assert_eq!(parse_raw_tls_request(&buf), Err(RawTlsError::InvalidLength));

    // Oversize hostname (> 495)
    buf[15..17].copy_from_slice(&500u16.to_le_bytes());
    assert_eq!(parse_raw_tls_request(&buf), Err(RawTlsError::OversizePayload(500)));
}

#[test]
fn parse_send_preserves_binary_trailing_zeros() {
    let binary = b"test\x00\x00\x00\x00";
    let mut buf = alloc::vec![0u8; 11 + binary.len()];
    buf[0] = TLS_SEND_OP;
    buf[1..9].copy_from_slice(&77u64.to_le_bytes());
    buf[9..11].copy_from_slice(&(binary.len() as u16).to_le_bytes());
    buf[11..].copy_from_slice(binary);

    assert_eq!(
        parse_raw_tls_request(&buf),
        Ok(RawTlsRequest::Send {
            cap: 77,
            data: binary,
        })
    );
}

#[test]
fn parse_send_negative_cases() {
    // Buffer < 11
    let short = [TLS_SEND_OP, 1, 2, 3, 4, 5, 6, 7, 8, 0];
    assert_eq!(parse_raw_tls_request(&short), Err(RawTlsError::BufferTooShort));

    // Declared length > buffer available
    let mut bad_len = [0u8; 15];
    bad_len[0] = TLS_SEND_OP;
    bad_len[9..11].copy_from_slice(&100u16.to_le_bytes());
    assert_eq!(parse_raw_tls_request(&bad_len), Err(RawTlsError::InvalidLength));

    // Oversize payload (> 501)
    let mut oversize = alloc::vec![0u8; 600];
    oversize[0] = TLS_SEND_OP;
    oversize[9..11].copy_from_slice(&550u16.to_le_bytes());
    assert_eq!(parse_raw_tls_request(&oversize), Err(RawTlsError::OversizePayload(550)));
}

#[test]
fn parse_recv_exact_and_negative() {
    let mut buf = [0u8; 13];
    buf[0] = TLS_RECV_OP;
    buf[1..9].copy_from_slice(&99u64.to_le_bytes());
    buf[9..13].copy_from_slice(&512u32.to_le_bytes());

    assert_eq!(
        parse_raw_tls_request(&buf),
        Ok(RawTlsRequest::Recv {
            cap: 99,
            buf_len: 512,
        })
    );

    // Clamping to MAX_TLS_RECV_DATA (4094)
    buf[9..13].copy_from_slice(&8192u32.to_le_bytes());
    assert_eq!(
        parse_raw_tls_request(&buf),
        Ok(RawTlsRequest::Recv {
            cap: 99,
            buf_len: MAX_TLS_RECV_DATA,
        })
    );

    // Buffer < 13
    assert_eq!(parse_raw_tls_request(&buf[..12]), Err(RawTlsError::BufferTooShort));
}

#[test]
fn recv_reply_encoding_preserves_zeros() {
    let data = b"\x00\x00\x00binary\x00";
    let encoded = encode_tls_recv_reply(data);
    assert_eq!(encoded.len(), 2 + data.len());
    let len = u16::from_le_bytes([encoded[0], encoded[1]]) as usize;
    assert_eq!(len, data.len());
    assert_eq!(&encoded[2..], data);
}

#[test]
fn recv_reply_encoding_clamps_at_max_recv_data() {
    let large_data = alloc::vec![0xAA; 5000];
    let encoded = encode_tls_recv_reply(&large_data);
    assert_eq!(encoded.len(), 2 + MAX_TLS_RECV_DATA);
    assert!(encoded.len() <= 4096);
    let len = u16::from_le_bytes([encoded[0], encoded[1]]) as usize;
    assert_eq!(len, MAX_TLS_RECV_DATA);
}
