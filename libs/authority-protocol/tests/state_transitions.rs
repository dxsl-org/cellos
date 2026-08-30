mod support;
use authority_protocol::*;
use support::*;

fn activate(state: &mut TestState, challenges: &mut Challenges, boot: u64, digest: [u8; 32]) {
    grant_time(state, challenges, 2, boot, TimePurpose::Enrollment, 1, 200);
    let enrollment = state
        .begin_enrollment(&begin(4, boot), &Clock(101))
        .unwrap();
    assert_eq!(enrollment.generation, 1);
    assert_eq!(enrollment.pending_slot, 0);
    assert_eq!(enrollment.hostname.as_slice(), b"relay.example");
    complete_upload(state, 5, boot, 1, digest);
    state.consume_receipt(&consume(8, boot, 1, digest)).unwrap();
    promote(state, &commit(9, boot, 1, digest));
}

#[test]
fn complete_enrollment_path_reaches_active_and_one_shot_signing() {
    let digest = [9; 32];
    let mut authority = state(6, 0);
    let mut challenges = Challenges(4);
    assert_eq!(authority.open_boot(&open(1), &measurement()), Ok(7));
    assert_eq!(
        authority.opened_boot_fact(),
        Some(OpenedBootFact {
            boot_epoch: 7,
            state_epoch: 1,
            approved_loader_digest: [7; 32],
        })
    );
    activate(&mut authority, &mut challenges, 7, digest);

    grant_time(
        &mut authority,
        &mut challenges,
        10,
        7,
        TimePurpose::TlsCertificateVerify,
        2,
        250,
    );
    let signing = validated(SignTls13ClientCertificateVerifyRequest {
        context: context(12, 7, Operation::SignTls13ClientCertificateVerify),
        transcript_hash: [4; 32],
        relay_generation: 1,
        active_profile_digest: digest,
        public_request_id: 44,
    });
    assert_eq!(
        authority.authorize_tls_signature(&signing, &Clock(150)),
        Ok(TlsSignatureIntent {
            relay_generation: 1,
            transcript_hash: [4; 32],
            active_profile_digest: digest,
            public_request_id: 44,
        })
    );
    let second = validated(SignTls13ClientCertificateVerifyRequest {
        context: context(13, 7, Operation::SignTls13ClientCertificateVerify),
        transcript_hash: [5; 32],
        relay_generation: 1,
        active_profile_digest: digest,
        public_request_id: 45,
    });
    assert_eq!(
        authority.authorize_tls_signature(&second, &Clock(151)),
        Err(AuthorityFault::TimeUnavailable)
    );
}

#[test]
fn abort_before_prepare_restores_previous_active_generation() {
    let digest = [5; 32];
    let mut authority = state(0, 0);
    let mut challenges = Challenges(4);
    authority.open_boot(&open(1), &measurement()).unwrap();
    activate(&mut authority, &mut challenges, 1, digest);

    grant_time(
        &mut authority,
        &mut challenges,
        10,
        1,
        TimePurpose::Enrollment,
        2,
        250,
    );
    assert_eq!(
        authority
            .begin_enrollment(&begin(12, 1), &Clock(102))
            .map(|intent| intent.generation),
        Ok(2)
    );
    let abort = validated(AbortRelayEnrollmentRequest {
        context: context(13, 1, Operation::AbortRelayEnrollment),
        generation: 2,
    });
    authority.abort(&abort).unwrap();
    assert!(matches!(
        authority.relay_state(),
        RelayProfileState::Active(RelayIntent { generation: 1, .. })
    ));
}

#[test]
fn promoted_state_requires_finalize_and_cannot_abort() {
    let digest = [8; 32];
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
    complete_upload(&mut authority, 5, 1, 1, digest);
    authority
        .consume_receipt(&consume(8, 1, 1, digest))
        .unwrap();
    let prepared = authority.prepare_commit(&commit(9, 1, 1, digest)).unwrap();
    let intent = prepared.intent();
    let receipt = ProviderCasReceipt {
        device_id: intent.device_id,
        authority_id: intent.authority_id,
        authority_epoch: intent.authority_epoch,
        generation: 1,
        policy_epoch: 1,
        pending_slot: 0,
        pending_spki_digest: [7; 32],
        profile_digest: digest,
        boot_epoch: 1,
        validation_request_id: intent.validation_request_id,
        upload_handle: intent.upload_handle,
        profile_len: intent.profile_len,
        provider_signature: [9; 64],
    };
    let verified_receipt = verify_provider_cas_receipt(receipt, &CasPolicy).unwrap();
    authority
        .record_provider_promotion(&prepared, &verified_receipt)
        .unwrap();
    let abort = validated(AbortRelayEnrollmentRequest {
        context: context(10, 1, Operation::AbortRelayEnrollment),
        generation: 1,
    });
    assert_eq!(authority.abort(&abort), Err(AuthorityFault::InvalidState));
    assert_eq!(authority.mode(), AuthorityMode::Sealed);
}
