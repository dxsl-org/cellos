// SPDX-License-Identifier: MPL-2.0
//! Test-only deterministic transaction harness.
//!
//! NON-QUALIFYING: this in-memory fake has no independent persistence,
//! authentication, anti-replay, or physical failure domain. It must never be
//! used as evidence for a production external-floor backend.

use super::{
    BackendIdentity, FloorPortOutcome, FloorState, IntentDigest, SlotId, SlotObservation,
    TransactionId,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AdvanceOutcome {
    Committed(FloorState),
    AlreadyCommitted(FloorState),
    ConflictingIntent,
    WrongExpectedGeneration,
    Unavailable,
    Exhausted,
}

/// Every externally visible write/verify/advance/commit crash boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CrashBoundary {
    BeforeIntentWrite,
    AfterIntentWrite,
    AfterIntentVerify,
    BeforeFloorAdvance,
    AfterFloorAdvance,
    BeforeCommitWrite,
    AfterCommitWrite,
    AfterCommitVerify,
}

pub(super) struct NonQualifyingFakeFloor {
    state: FloorState,
    pub advance_calls: u32,
    pub successful_advances: u32,
    available: bool,
    exhausted: bool,
}

impl NonQualifyingFakeFloor {
    pub const fn new(state: FloorState) -> Self {
        Self {
            state,
            advance_calls: 0,
            successful_advances: 0,
            available: true,
            exhausted: false,
        }
    }

    pub fn make_unavailable(&mut self) {
        self.available = false;
    }

    pub fn make_exhausted(&mut self) {
        self.exhausted = true;
    }

    pub fn read(&self) -> FloorPortOutcome {
        if !self.available {
            FloorPortOutcome::Unavailable
        } else if self.exhausted {
            FloorPortOutcome::Exhausted
        } else {
            FloorPortOutcome::Authenticated(self.state)
        }
    }

    pub fn advance(
        &mut self,
        expected_generation: u64,
        transaction_id: TransactionId,
        intent_digest: IntentDigest,
    ) -> AdvanceOutcome {
        self.advance_calls += 1;
        if !self.available {
            return AdvanceOutcome::Unavailable;
        }
        if self.exhausted || expected_generation == u64::MAX {
            return AdvanceOutcome::Exhausted;
        }
        if expected_generation == self.state.generation {
            self.state = FloorState {
                generation: expected_generation + 1,
                transaction_id,
                intent_digest,
                backend_identity: self.state.backend_identity,
            };
            self.successful_advances += 1;
            return AdvanceOutcome::Committed(self.state);
        }
        if expected_generation.checked_add(1) == Some(self.state.generation) {
            if self.state.transaction_id == transaction_id
                && self.state.intent_digest == intent_digest
            {
                return AdvanceOutcome::AlreadyCommitted(self.state);
            }
            return AdvanceOutcome::ConflictingIntent;
        }
        AdvanceOutcome::WrongExpectedGeneration
    }
}

pub(super) struct TransactionHarness {
    pub floor: NonQualifyingFakeFloor,
    pub slots: [SlotObservation; 2],
}

impl TransactionHarness {
    pub fn baseline() -> Self {
        let current = state(1, 1);
        Self {
            floor: NonQualifyingFakeFloor::new(current),
            slots: [
                SlotObservation::AuthenticatedCommitted(current),
                SlotObservation::AuthenticatedCommitted(state(0, 0)),
            ],
        }
    }

    pub fn run_until(&mut self, boundary: CrashBoundary) {
        if boundary == CrashBoundary::BeforeIntentWrite {
            return;
        }
        let old = match self.floor.read() {
            FloorPortOutcome::Authenticated(state) => state,
            _ => return,
        };
        let next = FloorState {
            generation: old.generation + 1,
            transaction_id: transaction_id(2),
            intent_digest: intent_digest(2),
            backend_identity: old.backend_identity,
        };
        self.slots[index(SlotId::B)] = SlotObservation::AuthenticatedIntent(next);
        if matches!(
            boundary,
            CrashBoundary::AfterIntentWrite
                | CrashBoundary::AfterIntentVerify
                | CrashBoundary::BeforeFloorAdvance
        ) {
            return;
        }
        let _ = self
            .floor
            .advance(old.generation, next.transaction_id, next.intent_digest);
        if matches!(
            boundary,
            CrashBoundary::AfterFloorAdvance | CrashBoundary::BeforeCommitWrite
        ) {
            return;
        }
        self.slots[index(SlotId::B)] = SlotObservation::AuthenticatedCommitted(next);
    }
}

pub(super) const fn state(generation: u64, tag: u8) -> FloorState {
    FloorState {
        generation,
        transaction_id: transaction_id(tag),
        intent_digest: intent_digest(tag),
        backend_identity: backend_identity(7),
    }
}

pub(super) const fn transaction_id(tag: u8) -> TransactionId {
    [tag; 16]
}

pub(super) const fn intent_digest(tag: u8) -> IntentDigest {
    [tag; 32]
}

pub(super) const fn backend_identity(tag: u8) -> BackendIdentity {
    [tag; 16]
}

const fn index(slot: SlotId) -> usize {
    match slot {
        SlotId::A => 0,
        SlotId::B => 1,
    }
}
