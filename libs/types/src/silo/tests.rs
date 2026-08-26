use super::*;

#[test]
fn purpose_bound_requests_round_trip_canonically() {
    let status = DevelopmentSiloRequest::RelayStatus { request_seq: 1 };
    assert_eq!(DevelopmentSiloRequest::decode(&status.encode()), Some(status));
    let sign = DevelopmentSiloRequest::SignTls13ClientCertificateVerify {
        request_seq: 2,
        transcript_hash: [0x11; 32],
        relay_generation: DEVELOPMENT_RELAY_GENERATION,
        active_profile_digest: DEVELOPMENT_PROFILE_DIGEST,
        request_id: 9,
    };
    assert_eq!(DevelopmentSiloRequest::decode(&sign.encode()), Some(sign));
}

#[test]
fn zero_sequences_and_noncanonical_padding_are_rejected() {
    let zero = DevelopmentSiloRequest::RelayStatus { request_seq: 0 }.encode();
    assert_eq!(DevelopmentSiloRequest::decode(&zero), None);
    let mut padded = DevelopmentSiloRequest::RelayStatus { request_seq: 1 }.encode();
    padded[DEVELOPMENT_SILO_FRAME_LEN - 1] = 1;
    assert_eq!(DevelopmentSiloRequest::decode(&padded), None);
}

#[test]
fn responses_require_independent_nonzero_sequences_and_fixed_payloads() {
    let response = DevelopmentSiloResponse::Tls13ClientCertificateVerify {
        request_seq: 7,
        response_seq: 11,
        signature: [0x22; 64],
    };
    assert_eq!(DevelopmentSiloResponse::decode(&response.encode()), Some(response));
    let mut malformed = response.encode();
    malformed[100] = 1;
    assert_eq!(DevelopmentSiloResponse::decode(&malformed), None);
}

#[test]
fn error_responses_are_typed_and_canonical() {
    let response = DevelopmentSiloResponse::Error {
        request_seq: 3,
        response_seq: 4,
        error: DevelopmentSiloError::GuestFault,
    };
    assert_eq!(DevelopmentSiloResponse::decode(&response.encode()), Some(response));
}
