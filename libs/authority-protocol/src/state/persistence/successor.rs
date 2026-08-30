mod helpers;
use helpers::{enrolled, stable, time_floors_strictly_advance};

use super::ProtectedAuthorityRecord;
use crate::{AuthorityFault, AuthorityMode, BootState, RelayProfileState, TimeState};

/// Verify that `next` is one legal persisted Phase-2 transition after `previous`.
pub fn verify_protected_successor(
    previous: &ProtectedAuthorityRecord,
    next: &ProtectedAuthorityRecord,
) -> Result<(), AuthorityFault> {
    let valid = previous.invariants_hold()
        && next.invariants_hold()
        && previous.revision.checked_add(1) == Some(next.revision)
        && previous.device_id == next.device_id
        && previous.authority_id == next.authority_id
        && previous.authority_epoch == next.authority_epoch
        && previous.state_epoch == next.state_epoch
        && previous.approved_loader_digest == next.approved_loader_digest
        && previous.boot_floor <= next.boot_floor
        && previous.generation_floor <= next.generation_floor
        && previous.last_request_sequence <= next.last_request_sequence
        && state_advances(previous, next)
        && relay_advances(previous, next);
    valid
        .then_some(())
        .ok_or(AuthorityFault::PersistenceFailure)
}

fn state_advances(previous: &ProtectedAuthorityRecord, next: &ProtectedAuthorityRecord) -> bool {
    if previous.mode == AuthorityMode::Sealed {
        return next.mode == AuthorityMode::Sealed
            && previous.boot == next.boot
            && previous.time == next.time
            && previous.pending_time == next.pending_time
            && previous.time_floors == next.time_floors;
    }
    if next.mode == AuthorityMode::Sealed {
        return previous.boot == next.boot
            && next.time == TimeState::Unavailable
            && next.pending_time.is_none()
            && previous.time_floors == next.time_floors;
    }
    let boot_advanced = match next.boot {
        BootState::Open { epoch } => {
            epoch == next.boot_floor
                && previous.boot_floor.checked_add(1) == Some(epoch)
                && next.mode == AuthorityMode::Serving
                && previous.boot != next.boot
        }
        BootState::Closed => false,
    };
    if !boot_advanced && (previous.mode != next.mode || previous.boot != next.boot) {
        return false;
    }
    if boot_advanced {
        next.time == TimeState::Unavailable
            && next.pending_time.is_none()
            && previous.time_floors == next.time_floors
    } else {
        time_advances(previous, next)
    }
}

fn time_advances(previous: &ProtectedAuthorityRecord, next: &ProtectedAuthorityRecord) -> bool {
    if previous.time == next.time && previous.pending_time == next.pending_time {
        return previous.time_floors == next.time_floors;
    }
    match (
        previous.time,
        previous.pending_time,
        next.time,
        next.pending_time,
    ) {
        (TimeState::Unavailable, None, TimeState::Unavailable, Some(_)) => {
            previous.time_floors == next.time_floors
        }
        (
            TimeState::Unavailable,
            Some(challenge),
            TimeState::Valid {
                source_epoch,
                sequence,
                time_request_id,
                purpose,
                ..
            },
            None,
        ) => {
            time_request_id == challenge.time_request_id
                && purpose == challenge.purpose
                && source_epoch == next.time_floors.source_epoch
                && sequence == next.time_floors.source_sequence
                && time_floors_strictly_advance(previous.time_floors, next.time_floors)
        }
        (TimeState::Valid { .. }, None, TimeState::Unavailable, None) => {
            previous.time_floors == next.time_floors
        }
        _ => false,
    }
}

fn relay_advances(previous: &ProtectedAuthorityRecord, next: &ProtectedAuthorityRecord) -> bool {
    let old = previous.relay;
    let new = next.relay;
    if old == new && previous.previous_active == next.previous_active {
        return previous.generation_floor == next.generation_floor;
    }
    match (old, new) {
        (
            RelayProfileState::Empty,
            RelayProfileState::Pending {
                generation,
                pending_slot,
                ..
            },
        ) => enrolled(previous, next, None, generation, pending_slot),
        (
            RelayProfileState::Active(active),
            RelayProfileState::Pending {
                generation,
                pending_slot,
                ..
            },
        ) => enrolled(previous, next, Some(active), generation, pending_slot),
        (
            RelayProfileState::Pending {
                generation,
                csr_handle,
                pending_slot,
            },
            RelayProfileState::Uploading(upload),
        ) => {
            stable(previous, next)
                && generation == upload.generation
                && csr_handle == upload.csr_handle
                && pending_slot == upload.pending_slot
                && upload.next_index == 0
                && previous.previous_active == next.previous_active
        }
        (RelayProfileState::Uploading(old), RelayProfileState::Uploading(new)) => {
            stable(previous, next)
                && old.next_index.checked_add(1) == Some(new.next_index)
                && (crate::ProfileUploadIntent {
                    next_index: new.next_index,
                    ..old
                }) == new
                && previous.previous_active == next.previous_active
        }
        (RelayProfileState::Uploading(upload), RelayProfileState::Staged(intent)) => {
            stable(previous, next)
                && upload.complete()
                && upload.matches_relay_intent(&intent)
                && previous.previous_active == next.previous_active
        }
        (RelayProfileState::Staged(old), RelayProfileState::ReceiptConsumed(new))
        | (RelayProfileState::ReceiptConsumed(old), RelayProfileState::Prepared(new)) => {
            stable(previous, next) && old == new && previous.previous_active == next.previous_active
        }
        (RelayProfileState::Prepared(old), RelayProfileState::Promoted { intent: new, .. }) => {
            stable(previous, next) && old == new && previous.previous_active == next.previous_active
        }
        (RelayProfileState::Promoted { intent: old, .. }, RelayProfileState::Active(new)) => {
            stable(previous, next) && old == new && next.previous_active.is_none()
        }
        (
            RelayProfileState::Pending { .. }
            | RelayProfileState::Uploading(_)
            | RelayProfileState::Staged(_)
            | RelayProfileState::ReceiptConsumed(_),
            RelayProfileState::Empty,
        ) => {
            stable(previous, next)
                && previous.previous_active.is_none()
                && next.previous_active.is_none()
        }
        (
            RelayProfileState::Pending { .. }
            | RelayProfileState::Uploading(_)
            | RelayProfileState::Staged(_)
            | RelayProfileState::ReceiptConsumed(_),
            RelayProfileState::Active(new),
        ) => {
            stable(previous, next)
                && previous.previous_active == Some(new)
                && next.previous_active.is_none()
        }
        _ => false,
    }
}
