use crate::model::material::{key_only, staged_from_key};
use crate::{FullRecord, HardwareBindings, RecordError, SlotRole};
use authority_protocol::{verify_protected_successor, RelayProfileState};

impl FullRecord {
    /// Require an exact legal successor with non-regressing protected floors.
    pub fn validate_successor(&self, previous: Option<&Self>) -> Result<(), RecordError> {
        let Some(previous) = previous else {
            return require(
                self.counter == 1 && self.slot_role == SlotRole::A,
                RecordError::InvalidSuccessor,
            );
        };
        require(
            self.counter
                == previous
                    .counter
                    .checked_add(1)
                    .ok_or(RecordError::InvalidSuccessor)?
                && self.slot_role == previous.slot_role.other(),
            RecordError::InvalidSuccessor,
        )?;
        verify_protected_successor(&previous.protected, &self.protected)
            .map_err(|_| RecordError::InvalidSuccessor)?;
        validate_hardware(&previous.hardware, &self.hardware)?;
        require(
            profiles_advance(previous, self),
            RecordError::InvalidSuccessor,
        )
    }
}

fn validate_hardware(old: &HardwareBindings, new: &HardwareBindings) -> Result<(), RecordError> {
    require(
        old.lane_id == new.lane_id
            && old.approved_boot_measurement == new.approved_boot_measurement
            && old.approved_loader_digest == new.approved_loader_digest
            && old.manifest_key_digest == new.manifest_key_digest
            && old.trust_digest == new.trust_digest
            && old.verifier_digest == new.verifier_digest
            && old.denylist_digest == new.denylist_digest
            && old.qualification_digest == new.qualification_digest,
        RecordError::IdentityMismatch,
    )?;
    require(
        old.restart_floor <= new.restart_floor
            && old.firmware_floor <= new.firmware_floor
            && old.policy_floor <= new.policy_floor,
        RecordError::FloorRegression,
    )
}

fn profiles_advance(old: &FullRecord, new: &FullRecord) -> bool {
    let old_relay = old.protected.bindings().relay;
    let new_relay = new.protected.bindings().relay;
    if old_relay == new_relay {
        return old.active == new.active && old.pending == new.pending;
    }
    match (old_relay, new_relay) {
        (RelayProfileState::Empty, RelayProfileState::Pending { .. }) => {
            old.active.is_none()
                && old.pending.is_none()
                && new.active.is_none()
                && matches!(new.pending.as_ref(), Some(value) if key_only(value))
        }
        (RelayProfileState::Active(_), RelayProfileState::Pending { .. }) => {
            old.active == new.active
                && old.pending.is_none()
                && matches!(new.pending.as_ref(), Some(value) if key_only(value))
        }
        (RelayProfileState::Pending { .. }, RelayProfileState::Uploading(_))
        | (RelayProfileState::Uploading(_), RelayProfileState::Uploading(_)) => {
            old.active == new.active && old.pending == new.pending
        }
        (RelayProfileState::Uploading(_), RelayProfileState::Staged(_)) => {
            old.active == new.active
                && matches!(
                    (old.pending.as_ref(), new.pending.as_ref()),
                    (Some(old), Some(new)) if staged_from_key(old, new)
                )
        }
        (RelayProfileState::Staged(_), RelayProfileState::ReceiptConsumed(_))
        | (RelayProfileState::ReceiptConsumed(_), RelayProfileState::Prepared(_))
        | (RelayProfileState::Prepared(_), RelayProfileState::Promoted { .. }) => {
            old.active == new.active && old.pending == new.pending
        }
        (RelayProfileState::Promoted { .. }, RelayProfileState::Active(_)) => {
            new.active == old.pending && new.pending.is_none()
        }
        (
            RelayProfileState::Pending { .. }
            | RelayProfileState::Uploading(_)
            | RelayProfileState::Staged(_)
            | RelayProfileState::ReceiptConsumed(_),
            RelayProfileState::Empty,
        ) => new.active.is_none() && new.pending.is_none(),
        (
            RelayProfileState::Pending { .. }
            | RelayProfileState::Uploading(_)
            | RelayProfileState::Staged(_)
            | RelayProfileState::ReceiptConsumed(_),
            RelayProfileState::Active(_),
        ) => new.active == old.active && new.pending.is_none(),
        _ => false,
    }
}

fn require(condition: bool, error: RecordError) -> Result<(), RecordError> {
    condition.then_some(()).ok_or(error)
}
