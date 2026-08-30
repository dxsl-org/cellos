use super::ProtectedAuthorityRecord;
use crate::{
    AuthorityFault, AuthorityMode, BootState, ProtectedTimeFloors, RelayIntent, RelayProfileState,
    TimeState,
};

/// Verify that `next` is one legal persisted Phase-2 transition after `previous`.
pub fn verify_protected_successor(
    previous: &ProtectedAuthorityRecord,
    next: &ProtectedAuthorityRecord,
) -> Result<(), AuthorityFault> {
    let valid = previous.invariants_hold()
        && next.invariants_hold()
        && next.revision == previous.revision.checked_add(1).unwrap_or(0)
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
        (RelayProfileState::Empty, RelayProfileState::Pending { generation, .. }) => {
            enrolled(previous, next, None, generation)
        }
        (RelayProfileState::Active(active), RelayProfileState::Pending { generation, .. }) => {
            enrolled(previous, next, Some(active), generation)
        }
        (RelayProfileState::Pending { generation, .. }, RelayProfileState::Staged(intent)) => {
            stable(previous, next)
                && generation == intent.generation
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

fn enrolled(
    previous: &ProtectedAuthorityRecord,
    next: &ProtectedAuthorityRecord,
    active: Option<RelayIntent>,
    generation: u64,
) -> bool {
    previous.generation_floor.checked_add(1) == Some(generation)
        && next.generation_floor == generation
        && next.previous_active == active
}

fn stable(previous: &ProtectedAuthorityRecord, next: &ProtectedAuthorityRecord) -> bool {
    previous.generation_floor == next.generation_floor
}

fn time_floors_strictly_advance(old: ProtectedTimeFloors, new: ProtectedTimeFloors) -> bool {
    new.unix_seconds > old.unix_seconds
        && (new.source_epoch > old.source_epoch
            || (new.source_epoch == old.source_epoch && new.source_sequence > old.source_sequence))
}
