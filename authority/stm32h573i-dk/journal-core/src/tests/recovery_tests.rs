use super::support::*;
use crate::*;

#[test]
fn one_exact_current_slot_recovers_while_old_or_torn_slot_is_ignored() {
    let current = encoded(SlotRole::A);
    let torn = &current[..current.len() / 2];
    let recovered = recover(1, [Some(&current), Some(torn)], &TestAuth, &identity()).unwrap();
    assert_eq!(recovered.record().slot_role, SlotRole::A);
    assert_eq!(recovered.record().counter, 1);
}

#[test]
fn genesis_record_in_slot_b_seals_even_when_role_matches() {
    let genesis = encoded(SlotRole::B);
    assert_eq!(
        recover(1, [None, Some(&genesis)], &TestAuth, &identity()),
        Err(RecoveryError::Sealed)
    );
}

#[test]
fn current_record_requires_its_authenticated_exact_predecessor() {
    let prior = full_record(SlotRole::A);
    let current = successor(&prior);
    let prior_bytes = encode_full(&prior);
    let current_bytes = encode_full(&current);
    let recovered = recover(
        2,
        [Some(&prior_bytes), Some(&current_bytes)],
        &TestAuth,
        &identity(),
    )
    .unwrap();
    assert_eq!(recovered.record(), &current);
    assert_eq!(
        recover(2, [None, Some(&current_bytes)], &TestAuth, &identity()),
        Err(RecoveryError::Sealed)
    );
}

#[test]
fn individually_valid_but_illegal_successor_seals() {
    let prior = full_record(SlotRole::A);
    let mut current = successor(&prior);
    current.hardware.manifest_key_digest = [0x55; 32];
    assert_eq!(current.validate(), Ok(()));
    assert_eq!(
        recover(
            2,
            [Some(&encode_full(&prior)), Some(&encode_full(&current))],
            &TestAuth,
            &identity(),
        ),
        Err(RecoveryError::Sealed)
    );
}

#[test]
fn authenticated_identity_mismatch_cannot_hide_beside_valid_current() {
    let mut prior = full_record(SlotRole::A);
    let current = successor(&prior);
    prior.hardware.lane_id = [0x44; 32];
    assert_eq!(
        recover(
            2,
            [Some(&encode_full(&prior)), Some(&encode_full(&current))],
            &TestAuth,
            &identity(),
        ),
        Err(RecoveryError::Sealed)
    );
}

#[test]
fn authenticated_nonchain_counter_seals_instead_of_being_skipped() {
    let stale = record_at(1, SlotRole::A);
    let current = record_at(3, SlotRole::B);
    assert_eq!(
        recover(
            3,
            [Some(&encode_full(&stale)), Some(&encode_full(&current))],
            &TestAuth,
            &identity(),
        ),
        Err(RecoveryError::Sealed)
    );
}

#[test]
fn authenticated_slot_role_mismatch_cannot_hide_beside_valid_current() {
    let wrong_role_prior = record_at(1, SlotRole::B);
    let current = successor(&full_record(SlotRole::A));
    assert_eq!(
        recover(
            2,
            [
                Some(&encode_full(&wrong_role_prior)),
                Some(&encode_full(&current)),
            ],
            &TestAuth,
            &identity(),
        ),
        Err(RecoveryError::Sealed)
    );
}
#[test]
fn stale_future_and_wrong_identity_records_seal() {
    let current = encoded(SlotRole::A);
    assert_eq!(
        recover(2, [Some(&current), None], &TestAuth, &identity()),
        Err(RecoveryError::Sealed)
    );
    let mut wrong = identity();
    wrong.device_id = [9; 32];
    assert_eq!(
        recover(1, [Some(&current), None], &TestAuth, &wrong),
        Err(RecoveryError::Sealed)
    );
}

#[test]
fn physical_slot_role_is_authenticated() {
    let role_a = encoded(SlotRole::A);
    assert_eq!(
        recover(1, [None, Some(&role_a)], &TestAuth, &identity()),
        Err(RecoveryError::Sealed)
    );
}

#[test]
fn two_valid_counter_matching_slots_are_ambiguous() {
    let role_a = encoded(SlotRole::A);
    let role_b = encoded(SlotRole::B);
    assert_eq!(
        recover(1, [Some(&role_a), Some(&role_b)], &TestAuth, &identity()),
        Err(RecoveryError::Ambiguous)
    );
}
