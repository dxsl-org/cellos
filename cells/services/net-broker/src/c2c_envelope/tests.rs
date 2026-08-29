use super::*;

fn sample<'a>(payload: &'a [u8]) -> C2cEnvelope<'a> {
    C2cEnvelope {
        kind: EnvelopeKind::Request,
        retry_class: RetryClass::Conditional,
        request_id: 7,
        src_node: CellNetId([0x11; 32]),
        dst_node: CellNetId([0x22; 32]),
        src_boot_epoch: 3,
        dst_server_epoch: ServerEpoch::new(4).unwrap(),
        cluster_id: ClusterId(5),
        service_id: 6,
        export_id: 8,
        relative_deadline: RelativeDeadline::new(9_000).unwrap(),
        payload,
    }
}

#[test]
fn canonical_frame_round_trips() {
    let mut frame = [0u8; MAX_C2C_FRAME];
    let expected = sample(b"request");
    let len = expected.encode(&mut frame).expect("encodes");
    assert_eq!(len, C2C_HEADER_LEN + 7);
    assert_eq!(decode(&frame[..len]), Ok(expected));
}
#[test]
fn maximum_payload_fits_every_non_streaming_hop() {
    assert!(MAX_C2C_PAYLOAD <= crate::local_ingress::MAX_REQUEST_BODY);
    assert!(MAX_C2C_FRAME <= NOISE_PLAINTEXT_CAP);
    assert_eq!(MAX_C2C_PAYLOAD, 3_712);
    assert_eq!(MAX_C2C_FRAME, 3_824);
    let payload = [0x5a; MAX_C2C_PAYLOAD];
    let mut frame = [0u8; MAX_C2C_FRAME];
    assert_eq!(sample(&payload).encode(&mut frame), Ok(MAX_C2C_FRAME));
    assert_eq!(decode(&frame).unwrap().payload, payload);
}

#[test]
fn rejects_truncation_trailing_bytes_and_oversize() {
    let mut frame = [0u8; MAX_C2C_FRAME + 1];
    let len = sample(b"x").encode(&mut frame).unwrap();
    assert_eq!(
        decode(&frame[..C2C_HEADER_LEN - 1]),
        Err(EnvelopeError::ShortBuffer)
    );
    assert_eq!(
        decode(&frame[..len + 1]),
        Err(EnvelopeError::LengthMismatch)
    );

    frame[4..6].copy_from_slice(&((MAX_C2C_PAYLOAD + 1) as u16).to_le_bytes());
    assert_eq!(decode(&frame[..len]), Err(EnvelopeError::PayloadTooLarge));

    let oversized = [0u8; MAX_C2C_PAYLOAD + 1];
    assert_eq!(
        sample(&oversized).encode(&mut frame),
        Err(EnvelopeError::PayloadTooLarge)
    );
    let mut short = [0u8; C2C_HEADER_LEN];
    assert_eq!(
        sample(b"x").encode(&mut short),
        Err(EnvelopeError::ShortBuffer)
    );
}

#[test]
fn rejects_unknown_and_noncanonical_header_values() {
    let mut frame = [0u8; MAX_C2C_FRAME];
    let len = sample(b"").encode(&mut frame).unwrap();

    frame[0] = 2;
    assert_eq!(
        decode(&frame[..len]),
        Err(EnvelopeError::UnsupportedVersion)
    );
    frame[0] = C2C_VERSION;
    frame[1] = 0xff;
    assert_eq!(decode(&frame[..len]), Err(EnvelopeError::UnknownKind));
    frame[1] = EnvelopeKind::Request as u8;
    frame[2] = 0xff;
    assert_eq!(decode(&frame[..len]), Err(EnvelopeError::UnknownRetryClass));
    frame[2] = RetryClass::Conditional as u8;
    frame[3] = 1;
    assert_eq!(decode(&frame[..len]), Err(EnvelopeError::NonCanonical));
}

#[test]
fn rejects_zero_authority_and_correlation_fields() {
    let mut frame = [0u8; MAX_C2C_FRAME];
    let mut envelope = sample(b"");
    envelope.request_id = 0;
    assert_eq!(
        envelope.encode(&mut frame),
        Err(EnvelopeError::InvalidIdentity)
    );

    envelope = sample(b"");
    envelope.src_node = CellNetId([0; 32]);
    assert_eq!(
        envelope.encode(&mut frame),
        Err(EnvelopeError::InvalidIdentity)
    );

    envelope = sample(b"");
    let len = envelope.encode(&mut frame).unwrap();
    frame[108..112].fill(0);
    assert_eq!(decode(&frame[..len]), Err(EnvelopeError::InvalidIdentity));
}

#[test]
fn every_accepted_single_byte_mutation_is_canonical() {
    let mut canonical = [0u8; MAX_C2C_FRAME];
    let len = sample(b"property").encode(&mut canonical).unwrap();

    for index in 0..len {
        for mask in [1u8, 0x80, 0xff] {
            let mut candidate = canonical;
            candidate[index] ^= mask;
            let Ok(decoded) = decode(&candidate[..len]) else {
                continue;
            };
            let mut encoded = [0u8; MAX_C2C_FRAME];
            let encoded_len = decoded.encode(&mut encoded).unwrap();
            assert_eq!(encoded_len, len);
            assert_eq!(&encoded[..len], &candidate[..len]);
        }
    }
}

#[test]
fn deterministic_hostile_frames_never_escape_canonical_reencode() {
    let mut state = 0x9e37_79b9u32;
    for len in 0..=C2C_HEADER_LEN + 16 {
        let mut candidate = [0u8; C2C_HEADER_LEN + 16];
        for byte in &mut candidate[..len] {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *byte = (state >> 24) as u8;
        }
        let Ok(decoded) = decode(&candidate[..len]) else {
            continue;
        };
        let mut encoded = [0u8; MAX_C2C_FRAME];
        let encoded_len = decoded.encode(&mut encoded).unwrap();
        assert_eq!(encoded_len, len);
        assert_eq!(&encoded[..len], &candidate[..len]);
    }
}
