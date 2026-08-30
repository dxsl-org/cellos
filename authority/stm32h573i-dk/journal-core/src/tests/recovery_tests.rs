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
