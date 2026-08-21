// SPDX-License-Identifier: MPL-2.0
//! Named bare-metal hostile-state tests for the pure admission decision.

use super::hostile::{backend_identity, state};
use super::{
    decide, AdmissionDecision, DenyReason, FloorPortOutcome, RecoveryReason, SlotId,
    SlotObservation,
};

fn committed(generation: u64, tag: u8) -> SlotObservation {
    SlotObservation::AuthenticatedCommitted(state(generation, tag))
}

pub(super) fn old_a_replay_admits_current_b_only() -> bool {
    decide(&FloorPortOutcome::Authenticated(state(2, 2)), &[committed(1, 1), committed(2, 2)]) == AdmissionDecision::Admit(SlotId::B)
}

pub(super) fn old_b_replay_admits_current_a_only() -> bool {
    decide(&FloorPortOutcome::Authenticated(state(2, 2)), &[committed(2, 2), committed(1, 1)]) == AdmissionDecision::Admit(SlotId::A)
}

pub(super) fn both_old_slots_deny() -> bool {
    decide(&FloorPortOutcome::Authenticated(state(3, 3)), &[committed(1, 1), committed(2, 2)]) == AdmissionDecision::RecoveryRequired(RecoveryReason::FloorAhead)
}

pub(super) fn stale_floor_response_denies() -> bool {
    decide(&FloorPortOutcome::Authenticated(state(1, 1)), &[committed(2, 2), committed(0, 0)]) == AdmissionDecision::RecoveryRequired(RecoveryReason::SlotAhead)
}

pub(super) fn wrong_transaction_binding_denies() -> bool {
    decide(&FloorPortOutcome::Authenticated(state(2, 2)), &[committed(2, 9), committed(1, 1)]) == AdmissionDecision::RecoveryRequired(RecoveryReason::ConflictingBinding)
}

pub(super) fn wrong_backend_binding_denies() -> bool {
    let mut foreign = state(1, 1);
    foreign.backend_identity = backend_identity(8);
    decide(&FloorPortOutcome::Authenticated(state(2, 2)), &[committed(2, 2), SlotObservation::AuthenticatedCommitted(foreign)]) == AdmissionDecision::RecoveryRequired(RecoveryReason::BackendMismatch)
}

pub(super) fn torn_uncommitted_slot_denies() -> bool {
    decide(&FloorPortOutcome::Authenticated(state(2, 2)), &[
        committed(1, 1),
        SlotObservation::AuthenticatedIntent(state(2, 2)),
    ]) == AdmissionDecision::RecoveryRequired(RecoveryReason::SlotUncommitted)
}

pub(super) fn missing_slot_denies() -> bool {
    decide(&FloorPortOutcome::Authenticated(state(2, 2)), &[committed(2, 2), SlotObservation::Missing]) == AdmissionDecision::RecoveryRequired(RecoveryReason::SlotMissing)
}

pub(super) fn invalid_slot_denies() -> bool {
    decide(&FloorPortOutcome::Authenticated(state(2, 2)), &[committed(2, 2), SlotObservation::Invalid]) == AdmissionDecision::RecoveryRequired(RecoveryReason::SlotInvalid)
}

pub(super) fn floor_ahead_denies() -> bool {
    decide(&FloorPortOutcome::Authenticated(state(9, 9)), &[committed(7, 7), committed(8, 8)]) == AdmissionDecision::RecoveryRequired(RecoveryReason::FloorAhead)
}

pub(super) fn slot_ahead_denies() -> bool {
    decide(&FloorPortOutcome::Authenticated(state(8, 8)), &[committed(9, 9), committed(7, 7)]) == AdmissionDecision::RecoveryRequired(RecoveryReason::SlotAhead)
}

pub(super) fn duplicate_current_slots_deny_as_ambiguous() -> bool {
    decide(&FloorPortOutcome::Authenticated(state(2, 2)), &[committed(2, 2), committed(2, 2)]) == AdmissionDecision::RecoveryRequired(RecoveryReason::AmbiguousCurrent)
}

fn floor_failure(outcome: FloorPortOutcome, reason: DenyReason) -> bool {
    decide(&outcome, &[committed(2, 2), committed(1, 1)])
        == AdmissionDecision::Deny(reason)
}

pub(super) fn missing_backend_denies() -> bool {
    floor_failure(FloorPortOutcome::Missing, DenyReason::FloorMissing)
}

pub(super) fn invalid_backend_evidence_denies() -> bool {
    floor_failure(FloorPortOutcome::Invalid, DenyReason::FloorInvalid)
}

pub(super) fn replaced_backend_denies() -> bool {
    floor_failure(FloorPortOutcome::Replaced, DenyReason::BackendReplaced)
}

pub(super) fn unavailable_backend_denies() -> bool {
    floor_failure(FloorPortOutcome::Unavailable, DenyReason::FloorUnavailable)
}

pub(super) fn exhausted_backend_denies() -> bool {
    floor_failure(FloorPortOutcome::Exhausted, DenyReason::FloorExhausted)
}

pub(super) fn run() -> bool {
    let mut ok = true;
    ok &= old_a_replay_admits_current_b_only();
    ok &= old_b_replay_admits_current_a_only();
    ok &= both_old_slots_deny();
    ok &= stale_floor_response_denies();
    ok &= wrong_transaction_binding_denies();
    ok &= wrong_backend_binding_denies();
    ok &= torn_uncommitted_slot_denies();
    ok &= missing_slot_denies();
    ok &= invalid_slot_denies();
    ok &= floor_ahead_denies();
    ok &= slot_ahead_denies();
    ok &= duplicate_current_slots_deny_as_ambiguous();
    ok &= missing_backend_denies();
    ok &= invalid_backend_evidence_denies();
    ok &= replaced_backend_denies();
    ok &= unavailable_backend_denies();
    ok &= exhausted_backend_denies();
    ok
}
