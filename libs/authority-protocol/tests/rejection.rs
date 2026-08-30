mod support;
use authority_protocol::*;
use support::*;

fn encoded(request: TypedRequest) -> ([u8; 1236], usize) {
    let mut payload = [0u8; FRAME_MAX_PAYLOAD];
    let payload_len = request.encode_payload(&mut payload).unwrap();
    let context = match request {
        TypedRequest::OpenBoot(value) => value.context,
        _ => panic!("test helper expects open boot"),
    };
    let header = FrameHeader {
        class: FrameClass::Request,
        operation: request.operation(),
        payload_len: payload_len as u16,
        request_id: context.request_id,
        authenticator: context.authenticator[..16].try_into().unwrap(),
    };
    let mut frame = [0u8; 1236];
    let length = encode_typed_request(header, &request, &mut frame).unwrap();
    (frame, length)
}

#[test]
fn envelope_mutations_fail_closed() {
    let request = TypedRequest::OpenBoot(OpenBootRequest {
        context: context(1, 0, Operation::OpenBoot),
        loader_digest: [7; 32],
    });
    let (frame, length) = encoded(request);
    assert!(decode_typed_request(&frame[..length]).is_ok());

    let mut mutant = frame;
    mutant[4] = 1;
    assert_eq!(
        decode_typed_request(&mutant[..length]),
        Err(WireError::UnsupportedVersion)
    );
    mutant = frame;
    mutant[10] = 1;
    assert_eq!(
        decode_typed_request(&mutant[..length]),
        Err(WireError::NonZeroReserved)
    );
    assert_eq!(
        decode_typed_request(&frame[..length - 1]),
        Err(WireError::Truncated)
    );
    assert_eq!(
        decode_typed_request(&frame[..length + 1]),
        Err(WireError::TrailingBytes)
    );
}

#[test]
fn typed_operation_and_canonical_body_are_enforced() {
    let request = TypedRequest::OpenBoot(OpenBootRequest {
        context: context(1, 0, Operation::OpenBoot),
        loader_digest: [7; 32],
    });
    let (frame, length) = encoded(request);
    let mut mutant = frame;
    mutant[7] = Operation::ReadCommittedRelayState as u8;
    assert!(decode_typed_request(&mutant[..length]).is_err());

    mutant = frame;
    let payload_operation = FRAME_HEADER_LEN + 120;
    mutant[payload_operation] = Operation::RequestSignedTime as u8;
    assert_eq!(
        decode_typed_request(&mutant[..length]),
        Err(WireError::UnknownOperation)
    );
}

#[test]
fn direct_request_verification_rejects_wrong_envelope_before_hashing() {
    let mut request = OpenBootRequest {
        context: context(1, 0, Operation::OpenBoot),
        loader_digest: [7; 32],
    };
    authenticate(&mut request);
    let valid_header = header(&request);
    let mut wrong_class = valid_header;
    wrong_class.class = FrameClass::Response;
    assert_eq!(
        verify_typed_request(request, &wrong_class, &RequestPolicy),
        Err(AuthorityFault::ChallengeMismatch)
    );
    let mut wrong_length = valid_header;
    wrong_length.payload_len -= 1;
    assert_eq!(
        verify_typed_request(request, &wrong_length, &RequestPolicy),
        Err(AuthorityFault::ChallengeMismatch)
    );
    request.context.operation = Operation::RequestSignedTime;
    assert_eq!(
        verify_typed_request(request, &valid_header, &RequestPolicy),
        Err(AuthorityFault::ChallengeMismatch)
    );
}

#[test]
fn malformed_der_and_noncanonical_profile_fields_are_rejected() {
    let fact = AcceptSignedTimeRequest {
        context: context(1, 1, Operation::AcceptSignedTime),
        time_request_id: [4; 16],
        purpose: 1,
        source_epoch: 1,
        source_sequence: 1,
        unix_seconds: 100,
        expires_at: 200,
        nonce: [5; 32],
        source_signature: Bounded::from_slice(&[0x30, 0]).unwrap(),
    };
    let mut payload = [0u8; FRAME_MAX_PAYLOAD];
    let length = TypedRequest::AcceptSignedTime(fact)
        .encode_payload(&mut payload)
        .unwrap();
    assert_eq!(
        TypedRequest::decode_payload(Operation::AcceptSignedTime, &payload[..length]),
        Err(WireError::InvalidLength)
    );

    let profile = ValidateAndStageRelayProfileRequest {
        pending_slot: 2,
        ..stage(2, 1, 1, [8; 32])
    };
    let length = TypedRequest::ValidateAndStageRelayProfile(profile)
        .encode_payload(&mut payload)
        .unwrap();
    assert_eq!(
        TypedRequest::decode_payload(Operation::ValidateAndStageRelayProfile, &payload[..length]),
        Err(WireError::InvalidLength)
    );
}

#[test]
fn upload_handles_lengths_indices_and_chunks_are_bounded() {
    let mut payload = [0u8; FRAME_MAX_PAYLOAD];
    let begin = BeginRelayProfileUploadRequest {
        upload_handle: 0,
        ..upload_request(2, 1, 1, [8; 32])
    };
    let length = TypedRequest::BeginRelayProfileUpload(begin)
        .encode_payload(&mut payload)
        .unwrap();
    assert_eq!(
        TypedRequest::decode_payload(Operation::BeginRelayProfileUpload, &payload[..length]),
        Err(WireError::InvalidLength)
    );

    let chunk = WriteRelayProfileChunkRequest {
        context: context(2, 1, Operation::WriteRelayProfileChunk),
        upload_handle: 1,
        chunk_index: PROFILE_MAX_CHUNKS as u8,
        chunk: Bounded::from_slice(&[1]).unwrap(),
    };
    let length = TypedRequest::WriteRelayProfileChunk(chunk)
        .encode_payload(&mut payload)
        .unwrap();
    assert_eq!(
        TypedRequest::decode_payload(Operation::WriteRelayProfileChunk, &payload[..length]),
        Err(WireError::InvalidLength)
    );
}
