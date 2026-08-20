#![cfg_attr(not(test), allow(dead_code))]

use super::backend::{StoreError, StoreIo};
use super::record::{JournalRecord, SlotId, SLOT_A_PATH, SLOT_B_PATH, STORE_DIR};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JournalLoad {
    Empty,
    Loaded,
    RollbackDetected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActiveSlot {
    pub(crate) slot: SlotId,
    pub(crate) record: JournalRecord,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JournalState {
    pub(crate) load: JournalLoad,
    pub(crate) active: Option<ActiveSlot>,
}

impl JournalState {
    pub(crate) fn empty() -> Self {
        Self {
            load: JournalLoad::Empty,
            active: None,
        }
    }

    pub(crate) fn load(io: &mut impl StoreIo) -> Result<Self, StoreError> {
        let slot_a = read_slot(io, SlotId::A, SLOT_A_PATH)?;
        let slot_b = read_slot(io, SlotId::B, SLOT_B_PATH)?;
        Ok(match (slot_a, slot_b) {
            (None, None) => Self::empty(),
            (Some(active), None) | (None, Some(active)) => Self {
                load: JournalLoad::Loaded,
                active: Some(active),
            },
            (Some(a), Some(b)) => match order_slots(a, b) {
                Some(active) => Self {
                    load: JournalLoad::Loaded,
                    active: Some(active),
                },
                None => Self {
                    load: JournalLoad::RollbackDetected,
                    active: None,
                },
            },
        })
    }

    pub(crate) fn persist_placeholder(
        &mut self,
        io: &mut impl StoreIo,
        policy_epoch: u64,
    ) -> Result<ActiveSlot, StoreError> {
        io.ensure_dir(STORE_DIR)?;
        let next_slot = self
            .active
            .as_ref()
            .map_or(SlotId::A, |active| active.slot.inactive());
        let blob_revision = self
            .active
            .as_ref()
            .map_or(1, |active| active.record.blob_revision.saturating_add(1));
        let previous = self
            .active
            .as_ref()
            .map_or([0; 32], |active| active.record.digest());
        let record = JournalRecord::placeholder(next_slot, blob_revision, policy_epoch, previous);
        let path = slot_path(next_slot);
        let bytes = record.encode();
        io.write_file(path, &bytes)?;
        let readback = io
            .read_file(path, JournalRecord::ENCODED_LEN)?
            .ok_or(StoreError::ReadbackMismatch)?;
        if readback != bytes {
            return Err(StoreError::ReadbackMismatch);
        }
        let active = ActiveSlot {
            slot: next_slot,
            record,
        };
        self.load = JournalLoad::Loaded;
        self.active = Some(active.clone());
        Ok(active)
    }
}

fn read_slot(
    io: &mut impl StoreIo,
    slot: SlotId,
    path: &str,
) -> Result<Option<ActiveSlot>, StoreError> {
    let Some(size) = io.stat(path)? else {
        return Ok(None);
    };
    if size as usize != JournalRecord::ENCODED_LEN {
        return Ok(None);
    }
    let bytes = io
        .read_file(path, JournalRecord::ENCODED_LEN)?
        .filter(|bytes| bytes.len() == JournalRecord::ENCODED_LEN);
    Ok(bytes
        .and_then(|bytes| JournalRecord::decode(&bytes, slot))
        .map(|record| ActiveSlot { slot, record }))
}

fn order_slots(a: ActiveSlot, b: ActiveSlot) -> Option<ActiveSlot> {
    let (older, newer) = if a.record.blob_revision <= b.record.blob_revision {
        (a, b)
    } else {
        (b, a)
    };
    if older.record.blob_revision == newer.record.blob_revision {
        return None;
    }
    (newer.record.previous_slot_digest == older.record.digest()).then_some(newer)
}

fn slot_path(slot: SlotId) -> &'static str {
    match slot {
        SlotId::A => SLOT_A_PATH,
        SlotId::B => SLOT_B_PATH,
    }
}
