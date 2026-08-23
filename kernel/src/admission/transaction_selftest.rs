// SPDX-License-Identifier: MPL-2.0
//! Named bare-metal tests for transaction crash boundaries and fake semantics.

use super::hostile::{
    intent_digest, state, transaction_id, AdvanceOutcome, CrashBoundary, NonQualifyingFakeFloor,
    TransactionHarness,
};
use super::{
    decide, report_selftest_case, AdmissionDecision, FloorPortOutcome, RecoveryReason, SlotId,
    SlotObservation,
};

fn boundary_case(
    boundary: CrashBoundary,
    decision: AdmissionDecision,
    successful_advances: u32,
) -> bool {
    let mut harness = TransactionHarness::baseline();
    harness.run_until(boundary);
    decide(&harness.floor.read(), &harness.slots) == decision
        && harness.floor.successful_advances == successful_advances
}

pub(super) fn power_loss_before_intent_write() -> bool {
    boundary_case(
        CrashBoundary::BeforeIntentWrite,
        AdmissionDecision::Admit(SlotId::A),
        0,
    )
}

pub(super) fn power_loss_after_intent_write() -> bool {
    boundary_case(
        CrashBoundary::AfterIntentWrite,
        AdmissionDecision::RecoveryRequired(RecoveryReason::SlotUncommitted),
        0,
    )
}

pub(super) fn power_loss_after_intent_verify() -> bool {
    boundary_case(
        CrashBoundary::AfterIntentVerify,
        AdmissionDecision::RecoveryRequired(RecoveryReason::SlotUncommitted),
        0,
    )
}

pub(super) fn power_loss_before_floor_advance() -> bool {
    boundary_case(
        CrashBoundary::BeforeFloorAdvance,
        AdmissionDecision::RecoveryRequired(RecoveryReason::SlotUncommitted),
        0,
    )
}

pub(super) fn power_loss_after_floor_advance() -> bool {
    boundary_case(
        CrashBoundary::AfterFloorAdvance,
        AdmissionDecision::RecoveryRequired(RecoveryReason::SlotUncommitted),
        1,
    )
}

pub(super) fn power_loss_before_commit_write() -> bool {
    boundary_case(
        CrashBoundary::BeforeCommitWrite,
        AdmissionDecision::RecoveryRequired(RecoveryReason::SlotUncommitted),
        1,
    )
}

pub(super) fn power_loss_after_commit_write() -> bool {
    boundary_case(
        CrashBoundary::AfterCommitWrite,
        AdmissionDecision::Admit(SlotId::B),
        1,
    )
}

pub(super) fn power_loss_after_commit_verify() -> bool {
    boundary_case(
        CrashBoundary::AfterCommitVerify,
        AdmissionDecision::Admit(SlotId::B),
        1,
    )
}

pub(super) fn duplicate_advance_is_exactly_once() -> bool {
    let mut floor = NonQualifyingFakeFloor::new(state(1, 1));
    let first = floor.advance(1, transaction_id(2), intent_digest(2));
    let duplicate = floor.advance(1, transaction_id(2), intent_digest(2));
    matches!(first, AdvanceOutcome::Committed(_))
        && matches!(duplicate, AdvanceOutcome::AlreadyCommitted(_))
        && floor.advance_calls == 2
        && floor.successful_advances == 1
}

pub(super) fn conflicting_advance_fails_closed() -> bool {
    let mut floor = NonQualifyingFakeFloor::new(state(1, 1));
    let _ = floor.advance(1, transaction_id(2), intent_digest(2));
    let conflict = floor.advance(1, transaction_id(3), intent_digest(3));
    conflict == AdvanceOutcome::ConflictingIntent && floor.successful_advances == 1
}

pub(super) fn wrong_expected_generation_fails_closed() -> bool {
    let mut floor = NonQualifyingFakeFloor::new(state(2, 2));
    floor.advance(0, transaction_id(3), intent_digest(3)) == AdvanceOutcome::WrongExpectedGeneration
        && floor.successful_advances == 0
}

pub(super) fn unavailable_advance_fails_closed() -> bool {
    let mut floor = NonQualifyingFakeFloor::new(state(1, 1));
    floor.make_unavailable();
    floor.advance(1, transaction_id(2), intent_digest(2)) == AdvanceOutcome::Unavailable
        && floor.successful_advances == 0
        && floor.read() == FloorPortOutcome::Unavailable
}

pub(super) fn exhausted_advance_fails_closed() -> bool {
    let mut floor = NonQualifyingFakeFloor::new(state(1, 1));
    floor.make_exhausted();
    floor.advance(1, transaction_id(2), intent_digest(2)) == AdvanceOutcome::Exhausted
        && floor.successful_advances == 0
        && floor.read() == FloorPortOutcome::Exhausted
}

pub(super) fn local_history_cannot_admit_or_advance_floor() -> bool {
    let floor = NonQualifyingFakeFloor::new(state(1, 1));
    let before = (floor.advance_calls, floor.successful_advances);
    let decision = decide(
        &floor.read(),
        &[
            SlotObservation::AuthenticatedCommitted(state(98, 8)),
            SlotObservation::AuthenticatedCommitted(state(99, 9)),
        ],
    );
    decision == AdmissionDecision::RecoveryRequired(RecoveryReason::SlotAhead)
        && before == (floor.advance_calls, floor.successful_advances)
        && floor.read() == FloorPortOutcome::Authenticated(state(1, 1))
}

type TransactionCase = (&'static str, fn() -> bool);

pub(super) fn run() -> bool {
    let cases: [TransactionCase; 14] = [
        ("C3-ADM-018", power_loss_before_intent_write),
        ("C3-ADM-019", power_loss_after_intent_write),
        ("C3-ADM-020", power_loss_after_intent_verify),
        ("C3-ADM-021", power_loss_before_floor_advance),
        ("C3-ADM-022", power_loss_after_floor_advance),
        ("C3-ADM-023", power_loss_before_commit_write),
        ("C3-ADM-024", power_loss_after_commit_write),
        ("C3-ADM-025", power_loss_after_commit_verify),
        ("C3-ADM-026", duplicate_advance_is_exactly_once),
        ("C3-ADM-027", conflicting_advance_fails_closed),
        ("C3-ADM-028", wrong_expected_generation_fails_closed),
        ("C3-ADM-029", unavailable_advance_fails_closed),
        ("C3-ADM-030", exhausted_advance_fails_closed),
        ("C3-ADM-031", local_history_cannot_admit_or_advance_floor),
    ];
    let mut ok = true;
    for (id, case) in cases {
        ok &= report_selftest_case(id, case());
    }
    ok
}
