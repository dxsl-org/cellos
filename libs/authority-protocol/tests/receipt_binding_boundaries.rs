mod support;

use authority_protocol::*;
use support::*;

const DIGEST: [u8; 32] = [9; 32];

fn staged_state() -> TestState {
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
    state
}

fn receipt_request(
    generation: u64,
    policy_epoch: u64,
    profile_digest: [u8; 32],
) -> ValidatedRequest<ConsumeStagedRelayProfileRequest> {
    validated(ConsumeStagedRelayProfileRequest {
        context: context(8, 1, Operation::ConsumeStagedRelayProfile),
        generation,
        policy_epoch,
        profile_digest,
    })
}

fn assert_receipt_rejected(generation: u64, policy_epoch: u64, profile_digest: [u8; 32]) {
    let mut state = staged_state();
    assert_eq!(
        state.consume_receipt(&receipt_request(generation, policy_epoch, profile_digest,)),
        Err(AuthorityFault::ReceiptAbsent)
    );
    assert_eq!(state.mode(), AuthorityMode::Sealed);
}

#[test]
fn staged_receipt_requires_exact_generation() {
    assert_receipt_rejected(2, 1, DIGEST);
}

#[test]
fn staged_receipt_requires_exact_policy_epoch() {
    assert_receipt_rejected(1, 2, DIGEST);
}

#[test]
fn staged_receipt_requires_exact_profile_digest() {
    assert_receipt_rejected(1, 1, [8; 32]);
}
