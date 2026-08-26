#![cfg_attr(not(test), allow(dead_code))]

use super::backend::{StoreError, StoreIo};
use super::record::{JournalKey, JournalRecord, SlotId, SLOT_A_PATH, SLOT_B_PATH, STORE_DIR};
use crate::lifecycle::ProtectedRelayState;

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

    pub(crate) fn load(io: &mut impl StoreIo, key: &JournalKey) -> Result<Self, StoreError> {
        let slot_a = read_slot(io, key, SlotId::A, SLOT_A_PATH)?;
        let slot_b = read_slot(io, key, SlotId::B, SLOT_B_PATH)?;
        Ok(match (slot_a, slot_b) {
            (None, None) => Self::empty(),
            (Some(active), None) | (None, Some(active)) => Self {
                load: JournalLoad::Loaded,
                active: Some(active),
            },
            (Some(a), Some(b)) => match order_slots(key, a, b) {
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
        key: &JournalKey,
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
            .map_or([0; 32], |active| active.record.digest(key));
        let record = JournalRecord::placeholder(next_slot, blob_revision, policy_epoch, previous);
        let path = slot_path(next_slot);
        let bytes = record.encode(key);
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

    pub(crate) fn protected_relay_state(&self) -> Option<ProtectedRelayState> {
        let active = self.active.as_ref()?;
        if active.record.payload_len as usize != ProtectedRelayState::ENCODED_LEN {
            return None;
        }
        ProtectedRelayState::decode(&active.record.sealed_leaf)
    }

    pub(crate) fn persist_relay_state(
        &mut self,
        io: &mut impl StoreIo,
        key: &JournalKey,
        protected: ProtectedRelayState,
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
            .map_or([0; 32], |active| active.record.digest(key));
        let policy_epoch = protected.active.map_or(0, |active| active.policy_epoch);
        let mut record = self
            .active
            .as_ref()
            .map(|active| active.record.clone())
            .unwrap_or_else(|| {
                JournalRecord::placeholder(next_slot, blob_revision, policy_epoch, previous)
            });
        record.slot = next_slot;
        record.blob_revision = blob_revision;
        record.policy_epoch = policy_epoch;
        record.payload_len = ProtectedRelayState::ENCODED_LEN as u16;
        record.sealed_leaf = protected.encode();
        record.previous_slot_digest = previous;
        let bytes = record.encode(key);
        let path = slot_path(next_slot);
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
    key: &JournalKey,
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
        .and_then(|bytes| JournalRecord::decode(&bytes, key, slot))
        .map(|record| ActiveSlot { slot, record }))
}

fn order_slots(key: &JournalKey, a: ActiveSlot, b: ActiveSlot) -> Option<ActiveSlot> {
    let (older, newer) = if a.record.blob_revision <= b.record.blob_revision {
        (a, b)
    } else {
        (b, a)
    };
    if older.record.blob_revision == newer.record.blob_revision {
        return None;
    }
    (newer.record.previous_slot_digest == older.record.digest(key)).then_some(newer)
}

fn slot_path(slot: SlotId) -> &'static str {
    match slot {
        SlotId::A => SLOT_A_PATH,
        SlotId::B => SLOT_B_PATH,
    }
}
