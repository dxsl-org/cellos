// SPDX-License-Identifier: MPL-2.0
//! Backend-neutral admission state core.
//!
//! This module is intentionally not connected to loader or boot admission. It
//! models the fail-closed decision that a future qualified backend may consume.

#[cfg(feature = "test-hooks")]
mod hostile;
#[cfg(feature = "test-hooks")]
mod state_selftest;
#[cfg(feature = "test-hooks")]
mod transaction_selftest;

pub(crate) type TransactionId = [u8; 16];
pub(crate) type IntentDigest = [u8; 32];
pub(crate) type BackendIdentity = [u8; 16];

/// Authenticated external-floor state. All fields form one inseparable binding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FloorState {
    pub generation: u64,
    pub transaction_id: TransactionId,
    pub intent_digest: IntentDigest,
    pub backend_identity: BackendIdentity,
}

/// Result taxonomy of the future external-floor port.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FloorPortOutcome {
    Authenticated(FloorState),
    Missing,
    Invalid,
    Unavailable,
    Exhausted,
    Replaced,
}

/// Authentication and commit state observed for a replaceable local slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SlotObservation {
    AuthenticatedCommitted(FloorState),
    AuthenticatedIntent(FloorState),
    Missing,
    Invalid,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SlotId {
    A,
    B,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DenyReason {
    FloorMissing,
    FloorInvalid,
    FloorUnavailable,
    FloorExhausted,
    BackendReplaced,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RecoveryReason {
    SlotMissing,
    SlotInvalid,
    SlotUncommitted,
    BackendMismatch,
    FloorAhead,
    SlotAhead,
    AmbiguousCurrent,
    ConflictingBinding,
}

/// Pure result: deliberately has no variant capable of advancing the floor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AdmissionDecision {
    Admit(SlotId),
    Deny(DenyReason),
    RecoveryRequired(RecoveryReason),
}

fn committed(slot: &SlotObservation) -> Result<&FloorState, RecoveryReason> {
    match slot {
        SlotObservation::AuthenticatedCommitted(state) => Ok(state),
        SlotObservation::AuthenticatedIntent(_) => Err(RecoveryReason::SlotUncommitted),
        SlotObservation::Missing => Err(RecoveryReason::SlotMissing),
        SlotObservation::Invalid => Err(RecoveryReason::SlotInvalid),
    }
}

/// Decide admission from authenticated observations only.
///
/// Admission requires exactly one current committed slot and one authenticated
/// stale committed partner from the same backend. Local bytes never select the
/// highest generation and this function has no floor-advance capability.
pub(crate) fn decide(
    floor: &FloorPortOutcome,
    slots: &[SlotObservation; 2],
) -> AdmissionDecision {
    let floor = match floor {
        FloorPortOutcome::Authenticated(state) => state,
        FloorPortOutcome::Missing => return AdmissionDecision::Deny(DenyReason::FloorMissing),
        FloorPortOutcome::Invalid => return AdmissionDecision::Deny(DenyReason::FloorInvalid),
        FloorPortOutcome::Unavailable => {
            return AdmissionDecision::Deny(DenyReason::FloorUnavailable)
        }
        FloorPortOutcome::Exhausted => return AdmissionDecision::Deny(DenyReason::FloorExhausted),
        FloorPortOutcome::Replaced => return AdmissionDecision::Deny(DenyReason::BackendReplaced),
    };
    let a = match committed(&slots[0]) {
        Ok(state) => state,
        Err(reason) => return AdmissionDecision::RecoveryRequired(reason),
    };
    let b = match committed(&slots[1]) {
        Ok(state) => state,
        Err(reason) => return AdmissionDecision::RecoveryRequired(reason),
    };
    if a.backend_identity != floor.backend_identity || b.backend_identity != floor.backend_identity {
        return AdmissionDecision::RecoveryRequired(RecoveryReason::BackendMismatch);
    }

    let a_current = a == floor;
    let b_current = b == floor;
    match (a_current, b_current) {
        (true, true) => AdmissionDecision::RecoveryRequired(RecoveryReason::AmbiguousCurrent),
        (true, false) => classify_partner(SlotId::A, b, floor),
        (false, true) => classify_partner(SlotId::B, a, floor),
        (false, false) => classify_mismatch(a, b, floor),
    }
}

fn classify_partner(current: SlotId, partner: &FloorState, floor: &FloorState) -> AdmissionDecision {
    if partner.generation < floor.generation {
        AdmissionDecision::Admit(current)
    } else if partner.generation > floor.generation {
        AdmissionDecision::RecoveryRequired(RecoveryReason::SlotAhead)
    } else {
        AdmissionDecision::RecoveryRequired(RecoveryReason::ConflictingBinding)
    }
}

fn classify_mismatch(a: &FloorState, b: &FloorState, floor: &FloorState) -> AdmissionDecision {
    if a.generation > floor.generation || b.generation > floor.generation {
        AdmissionDecision::RecoveryRequired(RecoveryReason::SlotAhead)
    } else if a.generation == floor.generation || b.generation == floor.generation {
        AdmissionDecision::RecoveryRequired(RecoveryReason::ConflictingBinding)
    } else {
        AdmissionDecision::RecoveryRequired(RecoveryReason::FloorAhead)
    }
}

#[cfg(feature = "test-hooks")]
fn report_selftest_case(id: &str, passed: bool) -> bool {
    if passed {
        log::info!("[selftest] {}: PASS", id);
    } else {
        log::info!("[selftest] {}: FAIL", id);
    }
    passed
}

#[cfg(feature = "test-hooks")]
pub(crate) fn self_test() -> bool {
    state_selftest::run() & transaction_selftest::run()
}
