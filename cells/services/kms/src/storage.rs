#![cfg_attr(not(test), allow(dead_code, unused_imports))]

mod backend;
mod journal;
mod record;
#[cfg(target_os = "none")]
mod runtime;

pub(crate) use backend::{StoreError, StoreIo};
pub(crate) use journal::{JournalLoad, JournalState};
pub(crate) use record::{JournalRecord, SlotId, SLOT_A_PATH, SLOT_B_PATH, STORE_DIR};

#[cfg(target_os = "none")]
use runtime::VfsJournalStore;

pub fn boot_probe_store() {
    #[cfg(target_os = "none")]
    {
        let mut io = VfsJournalStore::new();
        let _ = JournalState::load(&mut io).unwrap_or_else(|_| JournalState::empty());
    }
}

#[cfg(test)]
pub(crate) fn load_for_tests(io: &mut impl StoreIo) -> JournalState {
    JournalState::load(io).unwrap()
}

#[cfg(test)]
pub(crate) fn persist_placeholder_for_tests(
    state: &mut JournalState,
    io: &mut impl StoreIo,
    policy_epoch: u64,
) -> Result<(), backend::StoreError> {
    state.persist_placeholder(io, policy_epoch).map(|_| ())
}
