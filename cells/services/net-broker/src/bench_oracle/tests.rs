use super::*;

fn sample_counters() -> BrokerCounters {
    BrokerCounters {
        accepted: 1,
        completed: 2,
        busy: 3,
        terminal: 4,
        indeterminate: 5,
        duplicate: 6,
        stale_reply: 7,
        try_send_busy: 8,
        heartbeat_miss: 9,
        watchdog_expired: 0,
        peak_request_queue: 10,
        peak_reply_queue: 11,
        peak_in_flight: 12,
        peak_bytes: 13,
        network_progress: 14,
    }
}

#[test]
fn parses_echo_snapshot_and_hold_commands() {
    assert_eq!(
        parse_command(&[OP_ECHO, b'a']),
        Ok(OracleCommand::Echo(b"a"))
    );
    assert_eq!(parse_command(&[OP_SNAPSHOT]), Ok(OracleCommand::Snapshot));
    assert_eq!(
        parse_command(&[OP_HOLD, 0x34, 0x01]),
        Ok(OracleCommand::Hold { work_turns: 0x0134 })
    );
}

#[test]
fn rejects_empty_unknown_and_bad_hold_requests() {
    assert_eq!(parse_command(&[]), Err(OracleError::Empty));
    assert_eq!(parse_command(&[0x7e]), Err(OracleError::UnknownOpcode));
    assert_eq!(parse_command(&[OP_HOLD]), Err(OracleError::TruncatedHold));
    assert_eq!(
        parse_command(&[OP_HOLD, 0x01, 0x20]),
        Err(OracleError::HoldTooLarge)
    );
}

#[test]
fn encoders_enforce_bounds() {
    let mut out = [0u8; 4];
    assert_eq!(encode_snapshot_command(&mut out), Ok(1));
    assert_eq!(
        encode_hold_command(MAX_HOLD_TURNS + 1, &mut out),
        Err(OracleError::HoldTooLarge)
    );
    assert_eq!(
        encode_echo_command(b"abcd", &mut out),
        Err(OracleError::OutputTooSmall)
    );
}

#[test]
fn snapshot_payload_is_fixed_width_and_little_endian() {
    let mut out = [0u8; SNAPSHOT_BYTES];
    encode_snapshot_payload(&sample_counters(), 77, &mut out);
    assert_eq!(out[0], SNAPSHOT_VERSION);
    assert_eq!(u64::from_le_bytes(out[1..9].try_into().unwrap()), 1);
    assert_eq!(u64::from_le_bytes(out[9..17].try_into().unwrap()), 2);
    let last = 1 + (SNAPSHOT_U64_FIELDS - 1) * 8;
    assert_eq!(
        u64::from_le_bytes(out[last..last + 8].try_into().unwrap()),
        77
    );
    let decoded = decode_snapshot_payload(&out).expect("snapshot decodes");
    assert_eq!(decoded.network_progress, 14);
    assert_eq!(decoded.static_footprint_bytes, 77);
}

#[test]
fn request_and_reply_frames_round_trip() {
    let mut req = [0u8; api::ipc::IPC_BUF_SIZE];
    let req_len = encode_echo_request(9, b"abc", &mut req).expect("frame encodes");
    assert_eq!(u64::from_le_bytes(req[..8].try_into().unwrap()), 9);
    assert_eq!(u16::from_le_bytes(req[8..10].try_into().unwrap()), 4);
    assert_eq!(&req[10..req_len], &[OP_ECHO, b'a', b'b', b'c']);

    let mut reply = [0u8; api::ipc::IPC_BUF_SIZE];
    let reply_len = crate::local_ingress::encode_reply(
        crate::local_ingress::ReplyStatus::Busy,
        17,
        9,
        b"xy",
        &mut reply,
    );
    let decoded = decode_reply_frame(&reply[..reply_len]).expect("reply decodes");
    assert_eq!(decoded.status, crate::local_ingress::ReplyStatus::Busy);
    assert_eq!(decoded.request_id, 17);
    assert_eq!(decoded.client_sequence, 9);
    assert_eq!(decoded.payload, b"xy");
}

#[test]
fn reply_decoder_rejects_bad_frames() {
    assert_eq!(decode_reply_frame(&[]), Err(OracleError::InvalidReplyTag));
    let mut reply = [0u8; api::ipc::IPC_BUF_SIZE];
    reply[0] = crate::local_ingress::STATUS_TAG;
    reply[1] = 9;
    assert_eq!(
        decode_reply_frame(&reply[..crate::local_ingress::RESPONSE_HEADER_LEN]),
        Err(OracleError::InvalidReplyStatus)
    );
}

#[test]
fn static_footprint_stays_below_cap() {
    assert!(STATIC_FOOTPRINT_BYTES <= STATIC_FOOTPRINT_LIMIT_BYTES);
}
