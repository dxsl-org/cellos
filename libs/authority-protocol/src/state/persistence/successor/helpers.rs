use super::super::ProtectedAuthorityRecord;
use crate::{ProtectedTimeFloors, RelayIntent};

pub(super) fn enrolled(
    previous: &ProtectedAuthorityRecord,
    next: &ProtectedAuthorityRecord,
    active: Option<RelayIntent>,
    generation: u64,
    pending_slot: u8,
) -> bool {
    previous.generation_floor.checked_add(1) == Some(generation)
        && next.generation_floor == generation
        && pending_slot == active.map_or(0, |intent| intent.pending_slot ^ 1)
        && next.previous_active == active
}

pub(super) fn stable(previous: &ProtectedAuthorityRecord, next: &ProtectedAuthorityRecord) -> bool {
    previous.generation_floor == next.generation_floor
}

pub(super) fn time_floors_strictly_advance(
    old: ProtectedTimeFloors,
    new: ProtectedTimeFloors,
) -> bool {
    new.unix_seconds > old.unix_seconds
        && (new.source_epoch > old.source_epoch
            || (new.source_epoch == old.source_epoch && new.source_sequence > old.source_sequence))
}
