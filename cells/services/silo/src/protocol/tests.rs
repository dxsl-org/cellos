use super::*;
use types::silo::{
    DevelopmentSiloError as Error, DevelopmentSiloRequest as Request,
    DevelopmentSiloResponse as Response, DEVELOPMENT_PROFILE_DIGEST,
    DEVELOPMENT_RELAY_GENERATION,
};

#[derive(Clone, Copy)]
enum GuestFailure { Fault, Reset }

struct Guest {
    calls: usize,
    failure: Option<GuestFailure>,
}

impl Guest {
    fn ready() -> Self { Self { calls: 0, failure: None } }
}

impl PurposeGuest for Guest {
    type Error = GuestFailure;

    fn public_key(&self) -> [u8; 65] {
        let mut key = [0x22; 65];
        key[0] = 4;
        key
    }

    fn sign_tls13_client_certificate_verify(
        &mut self,
        _transcript_hash: [u8; 32],
    ) -> Result<[u8; 64], Self::Error> {
        self.calls += 1;
        self.failure.map_or(Ok([0x33; 64]), Err)
    }
}

fn peer(tid: usize) -> PeerIdentity {
    PeerIdentity { sender_tid: tid, cell_id: 91, generation: 3 }
}

fn sign(seq: u64) -> [u8; types::silo::DEVELOPMENT_SILO_FRAME_LEN] {
    Request::SignTls13ClientCertificateVerify {
        request_seq: seq,
        transcript_hash: [0x44; 32],
        relay_generation: DEVELOPMENT_RELAY_GENERATION,
        active_profile_digest: DEVELOPMENT_PROFILE_DIGEST,
        request_id: 7,
    }
    .encode()
}

fn failure(response: Option<Response>) -> Error {
    match response {
        Some(Response::Error { error, .. }) => error,
        other => panic!("expected typed failure, got {other:?}"),
    }
}

#[test]
fn direct_non_kms_is_denied_before_decode_or_provider_access() {
    let mut state = ProtocolState::new();
    let mut guest = Guest::ready();
    let invalid = [0xff; types::silo::DEVELOPMENT_SILO_FRAME_LEN];
    assert_eq!(failure(state.process(&mut guest, 8, Some(9), Some(peer(8)), &invalid)), Error::Unauthorized);
    assert_eq!(guest.calls, 0);

    let response = state.process(&mut guest, 9, Some(9), Some(peer(9)), &sign(1));
    assert!(matches!(response, Some(Response::Tls13ClientCertificateVerify { response_seq: 1, .. })));
    assert_eq!(guest.calls, 1);
}

#[test]
fn absent_or_forged_attestation_is_denied_without_state_mutation() {
    let mut state = ProtocolState::new();
    let mut guest = Guest::ready();
    assert_eq!(failure(state.process(&mut guest, 9, Some(9), None, &sign(1))), Error::Unauthorized);
    assert_eq!(failure(state.process(&mut guest, 9, Some(9), Some(peer(8)), &sign(1))), Error::Unauthorized);
    assert_eq!(guest.calls, 0);
    let response = state.process(&mut guest, 9, Some(9), Some(peer(9)), &sign(1));
    assert!(matches!(response, Some(Response::Tls13ClientCertificateVerify { response_seq: 1, .. })));
}

#[test]
fn malformed_and_stale_frames_are_typed_and_never_reach_guest() {
    let mut state = ProtocolState::new();
    let mut guest = Guest::ready();
    let mut malformed = sign(1);
    *malformed.last_mut().unwrap() = 1;
    assert_eq!(failure(state.process(&mut guest, 9, Some(9), Some(peer(9)), &malformed)), Error::Malformed);
    assert_eq!(guest.calls, 0);
    assert!(matches!(state.process(&mut guest, 9, Some(9), Some(peer(9)), &sign(2)), Some(Response::Tls13ClientCertificateVerify { response_seq: 2, .. })));
    assert_eq!(failure(state.process(&mut guest, 9, Some(9), Some(peer(9)), &sign(2))), Error::Sequence);
    assert_eq!(guest.calls, 1);
}

#[test]
fn responses_are_canonical_and_noncanonical_response_is_rejected() {
    let mut state = ProtocolState::new();
    let mut guest = Guest::ready();
    let response = state.process(&mut guest, 9, Some(9), Some(peer(9)), &sign(1)).unwrap();
    let mut frame = response.encode();
    assert_eq!(Response::decode(&frame), Some(response));
    *frame.last_mut().unwrap() = 1;
    assert_eq!(Response::decode(&frame), None);
}

#[test]
fn guest_fault_is_permanent_and_never_retried() {
    let mut state = ProtocolState::new();
    let mut guest = Guest { calls: 0, failure: Some(GuestFailure::Fault) };
    assert_eq!(failure(state.process(&mut guest, 9, Some(9), Some(peer(9)), &sign(1))), Error::GuestFault);
    assert_eq!(failure(state.process(&mut guest, 9, Some(9), Some(peer(9)), &sign(2))), Error::Unavailable);
    assert_eq!(guest.calls, 1);
}

#[test]
fn guest_reset_is_permanent_and_never_retried() {
    let mut state = ProtocolState::new();
    let mut guest = Guest { calls: 0, failure: Some(GuestFailure::Reset) };
    assert_eq!(failure(state.process(&mut guest, 9, Some(9), Some(peer(9)), &sign(1))), Error::GuestFault);
    assert_eq!(failure(state.process(&mut guest, 9, Some(9), Some(peer(9)), &sign(2))), Error::Unavailable);
    assert_eq!(guest.calls, 1);
}
