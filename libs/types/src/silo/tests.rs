use super::*;

#[test]
fn purpose_bound_requests_round_trip_canonically() {
    let status = DevelopmentSiloRequest::RelayStatus { request_seq: 1 };
    assert_eq!(
        DevelopmentSiloRequest::decode(&status.encode()),
        Some(status)
    );
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
    assert_eq!(
        DevelopmentSiloResponse::decode(&response.encode()),
        Some(response)
    );
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
    assert_eq!(
        DevelopmentSiloResponse::decode(&response.encode()),
        Some(response)
    );
}

#[test]
fn enrollment_commands_round_trip_canonically() {
    let create = DevelopmentSiloRequest::CreateEnrollmentKey {
        request_seq: 3,
        pending_generation: 2,
        nonce: [0x11; 32],
    };
    assert_eq!(
        DevelopmentSiloRequest::decode(&create.encode()),
        Some(create)
    );
    let mut hostname = [0u8; 64];
    hostname[..22].copy_from_slice(b"relay.example.internal");
    let cri = DevelopmentSiloRequest::SignEnrollmentCri {
        request_seq: 4,
        pending_generation: 2,
        hostname_len: 22,
        hostname,
    };
    assert_eq!(DevelopmentSiloRequest::decode(&cri.encode()), Some(cri));
    let destroy = DevelopmentSiloRequest::DestroyEnrollmentKey {
        request_seq: 5,
        pending_generation: 2,
    };
    assert_eq!(
        DevelopmentSiloRequest::decode(&destroy.encode()),
        Some(destroy)
    );
    let promote = DevelopmentSiloRequest::PromoteEnrollmentKey {
        request_seq: 6,
        pending_generation: 2,
        active_profile_digest: [0x77; 32],
    };
    assert_eq!(
        DevelopmentSiloRequest::decode(&promote.encode()),
        Some(promote)
    );
}

#[test]
fn enrollment_payload_rules_reject_degenerate_frames() {
    // Zero nonce for create.
    let mut create = DevelopmentSiloRequest::CreateEnrollmentKey {
        request_seq: 3,
        pending_generation: 2,
        nonce: [0x11; 32],
    }
    .encode();
    create[32..64].fill(0);
    let decoded = DevelopmentSiloRequest::decode(&create);
    assert_eq!(
        decoded.and_then(DevelopmentSiloRequest::validate_enrollment),
        None
    );
    assert!(matches!(
        decoded,
        Some(DevelopmentSiloRequest::CreateEnrollmentKey { .. })
    ));
    // Trailing garbage behind the CRI-signature hostname.
    let mut cri = DevelopmentSiloRequest::SignEnrollmentCri {
        request_seq: 4,
        pending_generation: 2,
        hostname_len: 1,
        hostname: [b'a'; 64],
    }
    .encode();
    cri[100] = 1;
    assert_eq!(DevelopmentSiloRequest::decode(&cri), None);
    // Zero digest for promote.
    let mut promote = DevelopmentSiloRequest::PromoteEnrollmentKey {
        request_seq: 6,
        pending_generation: 2,
        active_profile_digest: [0x77; 32],
    }
    .encode();
    promote[32..64].fill(0);
    assert_eq!(
        DevelopmentSiloRequest::decode(&promote)
            .and_then(DevelopmentSiloRequest::validate_enrollment),
        None
    );
}

#[test]
fn enrollment_responses_round_trip_with_exact_payload_bounds() {
    let created = DevelopmentSiloResponse::EnrollmentKeyCreated {
        request_seq: 3,
        response_seq: 1,
        verifying_key_sec1: [0x22; 65],
    };
    assert_eq!(
        DevelopmentSiloResponse::decode(&created.encode()),
        Some(created)
    );
    let signed = DevelopmentSiloResponse::EnrollmentCriSigned {
        request_seq: 4,
        response_seq: 2,
        signature: [0x34; 64],
    };
    assert_eq!(
        DevelopmentSiloResponse::decode(&signed.encode()),
        Some(signed)
    );
    let promoted = DevelopmentSiloResponse::EnrollmentKeyPromoted {
        request_seq: 6,
        response_seq: 3,
        verifying_key_sec1: [0x55; 65],
    };
    assert_eq!(
        DevelopmentSiloResponse::decode(&promoted.encode()),
        Some(promoted)
    );
}
