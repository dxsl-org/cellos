mod support;

use authority_protocol::*;
use support::*;

const DIGEST: [u8; 32] = [9; 32];

fn consumed_state() -> TestState {
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
    complete_upload(&mut state, 5, 1, 1, DIGEST);
    state.consume_receipt(&consume(8, 1, 1, DIGEST)).unwrap();
    state
}

fn commit_request(
    sequence: u64,
    generation: u64,
    policy_epoch: u64,
    profile_digest: [u8; 32],
) -> ValidatedRequest<CommitRelayGenerationRequest> {
    validated(CommitRelayGenerationRequest {
        context: context(sequence, 1, Operation::CommitRelayGeneration),
        generation,
        policy_epoch,
        profile_digest,
    })
}

fn assert_commit_rejected(generation: u64, policy_epoch: u64, profile_digest: [u8; 32]) {
    let mut state = consumed_state();
    assert_eq!(
        state.prepare_commit(&commit_request(9, generation, policy_epoch, profile_digest,)),
        Err(AuthorityFault::ProviderSplitBrain)
    );
    assert_eq!(state.mode(), AuthorityMode::Sealed);
}

#[test]
fn commit_requires_exact_generation() {
    assert_commit_rejected(2, 1, DIGEST);
}

#[test]
fn commit_requires_exact_policy_epoch() {
    assert_commit_rejected(1, 2, DIGEST);
}

#[test]
fn commit_requires_exact_profile_digest() {
    assert_commit_rejected(1, 1, [8; 32]);
}

#[test]
fn exact_newer_sequence_retry_recovers_prepared_intent() {
    let mut state = consumed_state();
    let first = state
        .prepare_commit(&commit_request(9, 1, 1, DIGEST))
        .unwrap();
    let recovered = state
        .prepare_commit(&commit_request(10, 1, 1, DIGEST))
        .unwrap();
    assert_eq!(recovered, first);
    assert_eq!(
        state.relay_state(),
        RelayProfileState::Prepared(*first.intent())
    );
}
