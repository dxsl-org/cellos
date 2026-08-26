//! Promotion continuity and stale-tuple rejection on the Silo lane.
//!
//! The serving (generation, profile digest) tuple is dynamic: promotion
//! adopts the promoted tuple atomically and every TLS request must match
//! it, so stale development-lane requests fail closed afterwards.

use super::*;

const GEN2: u64 = DEVELOPMENT_RELAY_GENERATION + 1;
const NEW_DIGEST: [u8; 32] = [0x77; 32];

fn create_with_nonce(
    seq: u64,
    generation: u64,
    nonce: [u8; 32],
) -> [u8; types::silo::DEVELOPMENT_SILO_FRAME_LEN] {
    Request::CreateEnrollmentKey {
        request_seq: seq,
        pending_generation: generation,
        nonce,
    }
    .encode()
}

fn create(seq: u64, generation: u64) -> [u8; types::silo::DEVELOPMENT_SILO_FRAME_LEN] {
    create_with_nonce(seq, generation, [0x11; 32])
}

fn sign_cri(seq: u64, generation: u64) -> [u8; types::silo::DEVELOPMENT_SILO_FRAME_LEN] {
    let mut hostname = [0u8; 64];
    hostname[..22].copy_from_slice(b"relay.example.internal");
    Request::SignEnrollmentCri {
        request_seq: seq,
        pending_generation: generation,
        hostname_len: 22,
        hostname,
    }
    .encode()
}

fn destroy(seq: u64, generation: u64) -> [u8; types::silo::DEVELOPMENT_SILO_FRAME_LEN] {
    Request::DestroyEnrollmentKey {
        request_seq: seq,
        pending_generation: generation,
    }
    .encode()
}

fn promote(seq: u64, generation: u64) -> [u8; types::silo::DEVELOPMENT_SILO_FRAME_LEN] {
    Request::PromoteEnrollmentKey {
        request_seq: seq,
        pending_generation: generation,
        active_profile_digest: NEW_DIGEST,
    }
    .encode()
}

fn tls(
    seq: u64,
    generation: u64,
    digest: [u8; 32],
) -> [u8; types::silo::DEVELOPMENT_SILO_FRAME_LEN] {
    Request::SignTls13ClientCertificateVerify {
        request_seq: seq,
        transcript_hash: [0x44; 32],
        relay_generation: generation,
        active_profile_digest: digest,
        request_id: 7,
    }
    .encode()
}

#[test]
fn promotion_binds_dynamic_tuple_and_rejects_stale_requests() {
    let mut state = ProtocolState::new();
    let mut guest = Guest::ready();
    // Zero nonce or zero generation never reaches the guest.
    let mut bad = create(1, GEN2);
    // Zero the nonce (fixed payload begins after the 24-byte header).
    bad[32..64].fill(0);
    assert_eq!(
        failure(state.process(&mut guest, 9, Some(9), Some(peer(9)), &bad)),
        Error::Malformed
    );
    assert_eq!(guest.calls, 0);

    assert!(matches!(
        state.process(&mut guest, 9, Some(9), Some(peer(9)), &create(1, GEN2)),
        Some(Response::EnrollmentKeyCreated { .. })
    ));
    assert!(matches!(
        state.process(&mut guest, 9, Some(9), Some(peer(9)), &sign_cri(2, GEN2)),
        Some(Response::EnrollmentCriSigned {
            request_seq: _,
            response_seq: _,
            signature: [0x34, ..]
        })
    ));
    assert!(matches!(
        state.process(&mut guest, 9, Some(9), Some(peer(9)), &promote(3, GEN2)),
        Some(Response::EnrollmentKeyPromoted {
            verifying_key_sec1: [0x04, 0x55, ..],
            ..
        })
    ));

    // The retired development tuple no longer signs.
    assert_eq!(
        failure(state.process(
            &mut guest,
            9,
            Some(9),
            Some(peer(9)),
            &tls(4, DEVELOPMENT_RELAY_GENERATION, DEVELOPMENT_PROFILE_DIGEST)
        )),
        Error::GenerationMismatch
    );
    // The promoted tuple serves dynamically.
    assert!(matches!(
        state.process(
            &mut guest,
            9,
            Some(9),
            Some(peer(9)),
            &tls(5, GEN2, NEW_DIGEST)
        ),
        Some(Response::Tls13ClientCertificateVerify { .. })
    ));
}

#[test]
fn failed_promotion_keeps_prior_tuple_serving() {
    let mut state = ProtocolState::new();
    let mut guest = Guest::ready();
    // No pending key exists for this generation: promotion is refused.
    assert_eq!(
        failure(state.process(&mut guest, 9, Some(9), Some(peer(9)), &promote(1, GEN2))),
        Error::NoEnrollmentKey
    );
    assert_eq!(guest.calls, 1);
    // The prior tuple still serves; the later TLS request reaches the guest.
    assert!(matches!(
        state.process(
            &mut guest,
            9,
            Some(9),
            Some(peer(9)),
            &tls(2, DEVELOPMENT_RELAY_GENERATION, DEVELOPMENT_PROFILE_DIGEST)
        ),
        Some(Response::Tls13ClientCertificateVerify { .. })
    ));
    assert_eq!(guest.calls, 2);
}

#[test]
fn destroyed_pending_key_cannot_be_promoted() {
    let mut state = ProtocolState::new();
    let mut guest = Guest::ready();
    assert!(matches!(
        state.process(&mut guest, 9, Some(9), Some(peer(9)), &create(1, GEN2)),
        Some(Response::EnrollmentKeyCreated { .. })
    ));
    assert!(matches!(
        state.process(&mut guest, 9, Some(9), Some(peer(9)), &destroy(2, GEN2)),
        Some(Response::EnrollmentKeyDestroyed { .. })
    ));
    assert_eq!(
        failure(state.process(&mut guest, 9, Some(9), Some(peer(9)), &promote(3, GEN2))),
        Error::NoEnrollmentKey
    );
    // Development tuple remains the serving identity.
    assert_eq!(
        failure(state.process(
            &mut guest,
            9,
            Some(9),
            Some(peer(9)),
            &tls(4, GEN2, NEW_DIGEST)
        )),
        Error::GenerationMismatch
    );
}

#[test]
fn create_routes_each_nonzero_nonce_without_reusing_the_prior_value() {
    let mut state = ProtocolState::new();
    let mut guest = Guest::ready();
    let Some(Response::EnrollmentKeyCreated {
        verifying_key_sec1: first,
        ..
    }) = state.process(
        &mut guest,
        9,
        Some(9),
        Some(peer(9)),
        &create_with_nonce(1, GEN2, [0x11; 32]),
    )
    else {
        panic!("first create response");
    };
    assert_eq!(guest.pending, Some((GEN2, [0x11; 32])));
    assert!(matches!(
        state.process(&mut guest, 9, Some(9), Some(peer(9)), &destroy(2, GEN2)),
        Some(Response::EnrollmentKeyDestroyed { .. })
    ));
    let Some(Response::EnrollmentKeyCreated {
        verifying_key_sec1: second,
        ..
    }) = state.process(
        &mut guest,
        9,
        Some(9),
        Some(peer(9)),
        &create_with_nonce(3, GEN2, [0x22; 32]),
    )
    else {
        panic!("replacement create response");
    };
    assert_eq!(guest.pending, Some((GEN2, [0x22; 32])));
    assert_ne!(first, second);
}

#[test]
fn destroy_distinguishes_absence_from_execution_and_transport_failure() {
    for (mut guest, expected) in [
        (Guest::ready(), Error::NoEnrollmentKey),
        (Guest::faulting(GuestFailure::Fault), Error::GuestFault),
        (Guest::faulting(GuestFailure::Reset), Error::Unavailable),
    ] {
        let mut state = ProtocolState::new();
        assert_eq!(
            failure(state.process(&mut guest, 9, Some(9), Some(peer(9)), &destroy(1, GEN2),)),
            expected
        );
    }
}
