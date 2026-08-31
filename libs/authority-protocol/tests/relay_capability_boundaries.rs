mod support;

use authority_protocol::*;
use support::*;

const DIGEST: [u8; 32] = [9; 32];

fn pending_state() -> (TestState, Challenges) {
    let mut state = state(0, 0);
    let mut challenges = Challenges(4);
    state.open_boot(&open(1), &measurement()).unwrap();
    grant_time(
        &mut state,
        &mut challenges,
        2,
        1,
        TimePurpose::Enrollment,
        1,
        200,
    );
    state.begin_enrollment(&begin(4, 1), &Clock(101)).unwrap();
    (state, challenges)
}

fn uploading_state() -> (TestState, Challenges) {
    let (mut state, challenges) = pending_state();
    let upload = state
        .authorize_profile_upload(&begin_upload(5, 1, 1, DIGEST))
        .unwrap();
    state.acknowledge_profile_upload(&upload).unwrap();
    (state, challenges)
}

fn staged_state() -> (TestState, Challenges) {
    let (mut state, challenges) = pending_state();
    complete_upload(&mut state, 5, 1, 1, DIGEST);
    (state, challenges)
}

fn consumed_state() -> (TestState, Challenges) {
    let (mut state, challenges) = staged_state();
    state.consume_receipt(&consume(8, 1, 1, DIGEST)).unwrap();
    (state, challenges)
}

fn prepared_state() -> (TestState, Challenges, PreparedCommitIntent) {
    let (mut state, challenges) = consumed_state();
    let prepared = state.prepare_commit(&commit(9, 1, 1, DIGEST)).unwrap();
    (state, challenges, prepared)
}

fn promoted_state() -> (TestState, Challenges) {
    let (mut state, challenges, prepared) = prepared_state();
    let intent = prepared.intent();
    let receipt = ProviderCasReceipt {
        device_id: intent.device_id,
        authority_id: intent.authority_id,
        authority_epoch: intent.authority_epoch,
        generation: intent.generation,
        policy_epoch: intent.policy_epoch,
        pending_slot: intent.pending_slot,
        pending_spki_digest: intent.pending_spki_digest,
        profile_digest: intent.profile_digest,
        boot_epoch: intent.boot_epoch,
        validation_request_id: intent.validation_request_id,
        upload_handle: intent.upload_handle,
        profile_len: intent.profile_len,
        provider_signature: [9; 64],
    };
    let verified = verify_provider_cas_receipt(receipt, &CasPolicy).unwrap();
    state
        .record_provider_promotion(&prepared, &verified)
        .unwrap();
    (state, challenges)
}

fn assert_signing_rejected(
    mut state: TestState,
    mut challenges: Challenges,
    next_sequence: u64,
    source_sequence: u64,
) {
    grant_time(
        &mut state,
        &mut challenges,
        next_sequence,
        1,
        TimePurpose::TlsCertificateVerify,
        source_sequence,
        250,
    );
    let request = validated(SignTls13ClientCertificateVerifyRequest {
        context: context(
            next_sequence + 2,
            1,
            Operation::SignTls13ClientCertificateVerify,
        ),
        transcript_hash: [4; 32],
        relay_generation: 1,
        active_profile_digest: DIGEST,
        public_request_id: 44,
    });
    assert_eq!(
        state.authorize_tls_signature(&request, &Clock(150)),
        Err(AuthorityFault::InvalidState)
    );
    assert_eq!(state.mode(), AuthorityMode::Sealed);
}

#[test]
fn signing_rejects_every_non_active_relay_state() {
    let mut empty = state(0, 0);
    let empty_challenges = Challenges(4);
    empty.open_boot(&open(1), &measurement()).unwrap();
    assert_signing_rejected(empty, empty_challenges, 2, 1);

    let (pending, pending_challenges) = pending_state();
    assert_signing_rejected(pending, pending_challenges, 5, 2);

    let (uploading, uploading_challenges) = uploading_state();
    assert_signing_rejected(uploading, uploading_challenges, 6, 2);

    let (staged, staged_challenges) = staged_state();
    assert_signing_rejected(staged, staged_challenges, 8, 2);

    let (consumed, consumed_challenges) = consumed_state();
    assert_signing_rejected(consumed, consumed_challenges, 9, 2);

    let (prepared, prepared_challenges, _) = prepared_state();
    assert_signing_rejected(prepared, prepared_challenges, 10, 2);

    let (promoted, promoted_challenges) = promoted_state();
    assert_signing_rejected(promoted, promoted_challenges, 10, 2);
}

#[test]
fn staged_receipt_is_consumed_exactly_once() {
    let (mut state, _) = staged_state();
    state.consume_receipt(&consume(8, 1, 1, DIGEST)).unwrap();
    assert_eq!(
        state.consume_receipt(&consume(9, 1, 1, DIGEST)),
        Err(AuthorityFault::ReceiptConsumed)
    );
    assert_eq!(state.mode(), AuthorityMode::Sealed);
}
