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

fn consumed_state() -> (TestState, Challenges, RelayIntent) {
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
    (state, challenges, intent)
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

fn unrelated_record() -> ProtectedAuthorityRecord {
    let mut unrelated = state(0, 0);
    unrelated.open_boot(&open(1), &measurement()).unwrap();
    unrelated.into_store().into_record().unwrap()
}

fn reboot_open(sequence: u64) -> ValidatedRequest<OpenBootRequest> {
    let mut request = OpenBootRequest {
        context: context(sequence, 0, Operation::OpenBoot),
        loader_digest: [7; 32],
    };
    request.context.challenge = REBOOT_CHALLENGE;
    validated(request)
}

fn reboot_commit(sequence: u64) -> ValidatedRequest<CommitRelayGenerationRequest> {
    let mut request = CommitRelayGenerationRequest {
        context: context(sequence, 2, Operation::CommitRelayGeneration),
        generation: 1,
        policy_epoch: 1,
        profile_digest: DIGEST,
    };
    request.context.challenge = REBOOT_CHALLENGE;
    validated(request)
}

#[test]
fn reboot_from_consumed_receipt_prepares_the_exact_persisted_intent() {
    let (mut state, mut challenges, expected_intent) = consumed_state();
    grant_time(
        &mut state,
        &mut challenges,
        9,
        1,
        TimePurpose::TlsCertificateVerify,
        2,
        250,
    );
    assert!(matches!(state.time_state(), TimeState::Valid { .. }));

    let (record, expected_digest) = persisted(state);
    assert_eq!(
        verify_protected_record(unrelated_record(), &RecordPolicy(expected_digest)),
        Err(AuthorityFault::PersistenceFailure)
    );
    let mut recovered = restore(record, expected_digest);
    assert_eq!(recovered.boot_state(), BootState::Closed);
    assert_eq!(recovered.time_state(), TimeState::Unavailable);
    assert_eq!(
        recovered.relay_state(),
        RelayProfileState::ReceiptConsumed(expected_intent)
    );
    assert_eq!(recovered.open_boot(&reboot_open(11), &measurement()), Ok(2));
    let prepared = recovered.prepare_commit(&reboot_commit(12)).unwrap();
    assert_eq!(*prepared.intent(), expected_intent);
    assert_eq!(
        recovered.relay_state(),
        RelayProfileState::Prepared(expected_intent)
    );
}

#[test]
fn reboot_from_prepared_recovers_the_identical_intent_on_exact_retry() {
    let (mut state, mut challenges, expected_intent) = consumed_state();
    let original = state.prepare_commit(&commit(9, 1, 1, DIGEST)).unwrap();
    assert_eq!(*original.intent(), expected_intent);
    grant_time(
        &mut state,
        &mut challenges,
        10,
        1,
        TimePurpose::TlsCertificateVerify,
        2,
        250,
    );
    assert!(matches!(state.time_state(), TimeState::Valid { .. }));
    let (record, expected_digest) = persisted(state);
    let mut recovered = restore(record, expected_digest);
    assert_eq!(recovered.time_state(), TimeState::Unavailable);
    assert_eq!(
        recovered.relay_state(),
        RelayProfileState::Prepared(expected_intent)
    );
    assert_eq!(recovered.open_boot(&reboot_open(12), &measurement()), Ok(2));
    let retry = recovered.prepare_commit(&reboot_commit(13)).unwrap();
    assert_eq!(retry, original);
    assert_eq!(
        recovered.relay_state(),
        RelayProfileState::Prepared(expected_intent)
    );
}
