mod support;
use authority_protocol::*;
use support::*;

#[test]
fn invalid_der_time_and_profile_never_create_validation_tokens() {
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

    let mut profile = stage(2, 1, 1, [5; 32]);
    profile.pending_slot = 2;
    authenticate(&mut profile);
    assert_eq!(
        verify_root_profile(profile, &header(&profile), &RequestPolicy, &ProfilePolicy),
        Err(AuthorityFault::ProfileRejected)
    );
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
        [1; 32],
        [2; 32],
        1,
        0,
        0,
        0,
        [3; 32],
        floors,
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
