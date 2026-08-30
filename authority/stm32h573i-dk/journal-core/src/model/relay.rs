use super::material::{key_only, matches_intent, matches_upload_key};
use super::{ProfileMaterial, RecordError};
use authority_protocol::{BootState, ProtectedRecordBindings, RelayIntent, RelayProfileState};

pub(super) fn validate(
    bindings: ProtectedRecordBindings,
    active: Option<&ProfileMaterial>,
    pending: Option<&ProfileMaterial>,
) -> Result<(), RecordError> {
    match bindings.relay {
        RelayProfileState::Empty => {
            require(bindings.previous_active.is_none() && active.is_none() && pending.is_none())
        }
        RelayProfileState::Pending {
            generation,
            pending_slot,
            ..
        } => {
            require_pending_key(bindings, generation, pending_slot, pending)?;
            require_previous(bindings.previous_active, active)
        }
        RelayProfileState::Uploading(intent) => {
            require(matches!(pending, Some(value) if matches_upload_key(value, intent)))?;
            require_previous(bindings.previous_active, active)
        }
        RelayProfileState::Staged(intent)
        | RelayProfileState::ReceiptConsumed(intent)
        | RelayProfileState::Prepared(intent)
        | RelayProfileState::Promoted { intent, .. } => {
            require(matches!(pending, Some(value) if matches_intent(value, intent)))?;
            require_previous(bindings.previous_active, active)
        }
        RelayProfileState::Active(intent) => {
            require(bindings.previous_active.is_none())?;
            require(matches!(active, Some(value) if matches_intent(value, intent)))?;
            require(pending.is_none())
        }
    }
}

fn require_pending_key(
    bindings: ProtectedRecordBindings,
    generation: u64,
    pending_slot: u8,
    pending: Option<&ProfileMaterial>,
) -> Result<(), RecordError> {
    let boot_epoch = match bindings.boot {
        BootState::Open { epoch } => epoch,
        BootState::Closed => return Err(RecordError::ProfileMismatch),
    };
    require(matches!(
        pending,
        Some(value)
            if key_only(value)
                && value.device_id == bindings.device_id
                && value.authority_id == bindings.authority_id
                && value.authority_epoch == bindings.authority_epoch
                && value.boot_epoch == boot_epoch
                && value.generation == generation
                && value.slot == pending_slot
    ))
}

fn require_previous(
    previous: Option<RelayIntent>,
    active: Option<&ProfileMaterial>,
) -> Result<(), RecordError> {
    match previous {
        Some(intent) => require(matches!(active, Some(value) if matches_intent(value, intent))),
        None => require(active.is_none()),
    }
}

fn require(condition: bool) -> Result<(), RecordError> {
    condition.then_some(()).ok_or(RecordError::ProfileMismatch)
}
