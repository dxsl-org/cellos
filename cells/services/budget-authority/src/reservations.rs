//! Unique reservation key tracking and reservation lifecycle.

use super::ledger::{ApplicationLedger, LedgerError, ResourceVector};
use alloc::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReservationState {
    Reserved,
    Claimed { destination: [u8; 32] },
    Closing,
    Released,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReservationEntry {
    pub key: [u8; 16],
    pub vector: ResourceVector,
    pub state: ReservationState,
    pub epoch: u64,
}

pub struct ReservationTable {
    entries: BTreeMap<[u8; 16], ReservationEntry>,
}

impl ReservationTable {
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    /// Atomically reserve capacity for a unique create key K.
    pub fn reserve(
        &mut self,
        ledger: &mut ApplicationLedger,
        key: [u8; 16],
        vector: ResourceVector,
        epoch: u64,
    ) -> Result<(), LedgerError> {
        if self.entries.contains_key(&key) {
            return Err(LedgerError::UnauthorizedMutation);
        }
        ledger.allocate(&vector)?;
        self.entries.insert(
            key,
            ReservationEntry {
                key,
                vector,
                state: ReservationState::Reserved,
                epoch,
            },
        );
        Ok(())
    }

    pub fn get(&self, key: &[u8; 16]) -> Option<&ReservationEntry> {
        self.entries.get(key)
    }

    pub fn get_mut(&mut self, key: &[u8; 16]) -> Option<&mut ReservationEntry> {
        self.entries.get_mut(key)
    }
}
