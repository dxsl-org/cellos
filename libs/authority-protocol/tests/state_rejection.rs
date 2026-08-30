use core::cell::Cell;
mod support;
use authority_protocol::*;
use support::*;
struct CountingProfilePolicy<'a>(&'a Cell<usize>);

impl RootProfileVerifier for CountingProfilePolicy<'_> {
    fn verify_root_profile(&self, _: &AdmittedProfileValidation) -> bool {
        self.0.set(self.0.get() + 1);
        true
    }
}

#[test]
fn unauthenticated_profile_never_creates_adapter_token() {
    let profile = stage(2, 1, 1, [5; 32]);
    assert_eq!(
        verify_typed_request(profile, &header(&profile), &RequestPolicy),
        Err(AuthorityFault::ChallengeMismatch)
    );
}

#[test]
fn invalid_der_time_never_creates_validation_token() {
    let mut time = AcceptSignedTimeRequest {
        context: context(1, 1, Operation::AcceptSignedTime),
        time_request_id: [4; 16],
        purpose: TimePurpose::Enrollment as u8,
        source_epoch: 1,
        source_sequence: 1,
        unix_seconds: 100,
        expires_at: 200,
        nonce: [5; 32],
        source_signature: Bounded::from_slice(&[0x30, 0]).unwrap(),
    };
    authenticate(&mut time);
    assert_eq!(
        verify_signed_time(time, &header(&time), &RequestPolicy, &TimePolicy),
        Err(AuthorityFault::TimeInvalid)
    );
}

#[test]
fn replayed_profile_validation_never_reaches_adapter_and_exact_retry_keeps_receipt() {
    let digest = [5; 32];
    let calls = Cell::new(0);
    let policy = CountingProfilePolicy(&calls);
    let mut authority = state(0, 0);
    let mut challenges = Challenges(4);
    authority.open_boot(&open(1), &measurement()).unwrap();
    grant_time(
        &mut authority,
        &mut challenges,
        2,
        1,
        TimePurpose::Enrollment,
        1,
        200,
    );
    authority
        .begin_enrollment(&begin(4, 1), &Clock(101))
        .unwrap();
    let upload = authority
        .authorize_profile_upload(&begin_upload(5, 1, 1, digest))
        .unwrap();
    authority.acknowledge_profile_upload(&upload).unwrap();
    let chunk = authority
        .authorize_profile_chunk(&write_profile(6, 1, digest))
        .unwrap();
    authority.acknowledge_profile_chunk(&chunk).unwrap();

    let authenticated = validated(stage(7, 1, 1, digest));
    let admitted = authority.admit_profile_validation(&authenticated).unwrap();
    assert_eq!(admitted.authority_epoch(), 1);
    assert_eq!(admitted.csr_handle(), 1);
    let verified = verify_root_profile(admitted, &policy).unwrap();
    let first = authority.stage_profile(&verified).unwrap();
    assert_eq!(calls.get(), 1);

    let retry = validated(stage(8, 1, 1, digest));
    let admitted = authority.admit_profile_validation(&retry).unwrap();
    let verified = verify_root_profile(admitted, &policy).unwrap();
    assert_eq!(authority.stage_profile(&verified), Ok(first));
    assert_eq!(calls.get(), 2);

    assert_eq!(
        authority.admit_profile_validation(&authenticated),
        Err(AuthorityFault::Replay)
    );
    assert_eq!(calls.get(), 2);
}

#[test]
fn mismatched_time_challenge_seals_before_lease_issue() {
    let mut authority = state(0, 0);
    let mut challenges = Challenges(4);
    authority.open_boot(&open(1), &measurement()).unwrap();
    let request = validated(RequestSignedTimeRequest {
        context: context(2, 1, Operation::RequestSignedTime),
        purpose: TimePurpose::Enrollment as u8,
    });
    let challenge = authority
        .request_signed_time(&request, &mut challenges)
        .unwrap();
    let mut fact = AcceptSignedTimeRequest {
        context: context(3, 1, Operation::AcceptSignedTime),
        time_request_id: challenge.time_request_id,
        purpose: TimePurpose::TlsCertificateVerify as u8,
        source_epoch: 1,
        source_sequence: 1,
        unix_seconds: 100,
        expires_at: 200,
        nonce: challenge.nonce,
        source_signature: time_signature(),
    };
    authenticate(&mut fact);
    let verified = verify_signed_time(fact, &header(&fact), &RequestPolicy, &TimePolicy).unwrap();
    assert_eq!(
        authority.accept_time(&verified, &Clock(100)),
        Err(AuthorityFault::TimeInvalid)
    );
    assert_eq!(authority.mode(), AuthorityMode::Sealed);
}

#[test]
fn protected_time_floors_survive_lease_consumption() {
    let floors = ProtectedTimeFloors {
        source_epoch: 1,
        source_sequence: 8,
        unix_seconds: 108,
    };
    let mut authority = AuthorityState::new(
        MemoryStore::default(),
        AuthorityStateConfig {
            device_id: [1; 32],
            authority_id: [2; 32],
            authority_epoch: 1,
            boot_floor: 0,
            generation_floor: 0,
            state_epoch: 0,
            boot_challenge: [3; 32],
            time_floors: floors,
        },
    );
    let mut challenges = Challenges(4);
    authority.open_boot(&open(1), &measurement()).unwrap();
    let request = validated(RequestSignedTimeRequest {
        context: context(2, 1, Operation::RequestSignedTime),
        purpose: TimePurpose::Enrollment as u8,
    });
    let challenge = authority
        .request_signed_time(&request, &mut challenges)
        .unwrap();
    let mut fact = AcceptSignedTimeRequest {
        context: context(3, 1, Operation::AcceptSignedTime),
        time_request_id: challenge.time_request_id,
        purpose: 1,
        source_epoch: 1,
        source_sequence: 8,
        unix_seconds: 109,
        expires_at: 200,
        nonce: challenge.nonce,
        source_signature: time_signature(),
    };
    authenticate(&mut fact);
    let verified = verify_signed_time(fact, &header(&fact), &RequestPolicy, &TimePolicy).unwrap();
    assert_eq!(
        authority.accept_time(&verified, &Clock(100)),
        Err(AuthorityFault::Regression)
    );
    assert_eq!(authority.mode(), AuthorityMode::Sealed);
}

#[test]
fn forged_context_and_replay_are_rejected() {
    let mut request = OpenBootRequest {
        context: context(1, 0, Operation::OpenBoot),
        loader_digest: [7; 32],
    };
    authenticate(&mut request);
    request.context.authenticator = [0; 32];
    assert_eq!(
        verify_typed_request(request, &header(&request), &RequestPolicy),
        Err(AuthorityFault::ChallengeMismatch)
    );

    let mut authority = state(0, 0);
    authority.open_boot(&open(1), &measurement()).unwrap();
    let replay = validated(RequestSignedTimeRequest {
        context: context(1, 1, Operation::RequestSignedTime),
        purpose: 1,
    });
    assert_eq!(
        authority.request_signed_time(&replay, &mut Challenges(4)),
        Err(AuthorityFault::Replay)
    );
    assert_eq!(authority.mode(), AuthorityMode::Sealed);
}
