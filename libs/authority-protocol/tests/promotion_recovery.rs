mod support;

use authority_protocol::*;
use support::*;

const DIGEST: [u8; 32] = [9; 32];
const REBOOT_CHALLENGE: [u8; 32] = [4; 32];

struct RecordPolicy([u8; 32]);

impl ProtectedRecordVerifier for RecordPolicy {
    fn verify(&self, record: &ProtectedAuthorityRecord) -> bool {
        constant_time_eq(&self.0, &record.authentication_digest())
    }
}

fn promoted_state() -> (TestState, RelayIntent) {
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
    let intent = complete_upload(&mut state, 5, 1, 1, DIGEST);
    state.consume_receipt(&consume(8, 1, 1, DIGEST)).unwrap();
    let prepared = state.prepare_commit(&commit(9, 1, 1, DIGEST)).unwrap();
    let receipt = provider_receipt(prepared.intent());
    let verified = verify_provider_cas_receipt(receipt, &CasPolicy).unwrap();
    state
        .record_provider_promotion(&prepared, &verified)
        .unwrap();
    grant_time(
        &mut state,
        &mut challenges,
        10,
        1,
        TimePurpose::TlsCertificateVerify,
        2,
        250,
    );
    (state, intent)
}

fn provider_receipt(intent: &RelayIntent) -> ProviderCasReceipt {
    ProviderCasReceipt {
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
        provider_signature: [9; SIGNATURE_LEN],
    }
}

fn persisted(state: TestState) -> (ProtectedAuthorityRecord, [u8; 32]) {
    let record = state.into_store().into_record().unwrap();
    let expected_digest = record.authentication_digest();
    (record, expected_digest)
}

fn restore(record: ProtectedAuthorityRecord, expected_digest: [u8; 32]) -> TestState {
    let verified = verify_protected_record(record, &RecordPolicy(expected_digest)).unwrap();
    AuthorityState::restore(
        MemoryStore::from_record(record),
        &verified,
        REBOOT_CHALLENGE,
    )
}

fn reboot_open() -> ValidatedRequest<OpenBootRequest> {
    let mut request = OpenBootRequest {
        context: context(12, 0, Operation::OpenBoot),
        loader_digest: [7; 32],
    };
    request.context.challenge = REBOOT_CHALLENGE;
    validated(request)
}

#[test]
fn reboot_from_promoted_restores_authenticated_intent_and_fresh_boot_boundary() {
    let (promoted, expected_intent) = promoted_state();
    assert!(matches!(promoted.time_state(), TimeState::Valid { .. }));
    let (record, expected_digest) = persisted(promoted);

    let mut substituted = state(0, 0);
    substituted.open_boot(&open(1), &measurement()).unwrap();
    let substituted_record = substituted.into_store().into_record().unwrap();
    assert_eq!(
        verify_protected_record(substituted_record, &RecordPolicy(expected_digest)),
        Err(AuthorityFault::PersistenceFailure)
    );

    let mut recovered = restore(record, expected_digest);
    assert_eq!(recovered.boot_state(), BootState::Closed);
    assert_eq!(recovered.time_state(), TimeState::Unavailable);
    assert!(matches!(
        recovered.relay_state(),
        RelayProfileState::Promoted { intent, .. } if intent == expected_intent
    ));
    recovered.finalize_commit().unwrap();
    assert_eq!(
        recovered.relay_state(),
        RelayProfileState::Active(expected_intent)
    );

    let mut fresh_boot = restore(record, expected_digest);
    assert_eq!(fresh_boot.open_boot(&reboot_open(), &measurement()), Ok(2));
    let mut stale_boot = restore(record, expected_digest);
    assert_eq!(
        stale_boot.open_boot(&open(12), &measurement()),
        Err(AuthorityFault::ChallengeMismatch)
    );
    assert_eq!(stale_boot.mode(), AuthorityMode::Sealed);
}

#[test]
fn reboot_after_finalize_retains_the_exact_active_intent() {
    let (mut state, expected_intent) = promoted_state();
    state.finalize_commit().unwrap();
    let (record, expected_digest) = persisted(state);
    let recovered = restore(record, expected_digest);
    assert_eq!(recovered.mode(), AuthorityMode::Ready);
    assert_eq!(recovered.boot_state(), BootState::Closed);
    assert_eq!(recovered.time_state(), TimeState::Unavailable);
    assert_eq!(
        recovered.relay_state(),
        RelayProfileState::Active(expected_intent)
    );
}
