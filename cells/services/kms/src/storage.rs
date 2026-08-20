#![cfg_attr(not(test), allow(dead_code, unused_imports))]

mod backend;
mod journal;
mod record;
mod root;
#[cfg(target_os = "none")]
mod runtime;

pub(crate) use backend::{StoreError, StoreIo};
pub(crate) use journal::{JournalLoad, JournalState};
pub(crate) use record::{JournalKey, JournalRecord, SlotId, SLOT_A_PATH, SLOT_B_PATH, STORE_DIR};
pub(crate) use root::RootAssessment;

#[cfg(target_os = "none")]
use runtime::VfsJournalStore;

pub fn boot_probe_store() {
    #[cfg(target_os = "none")]
    {
        let mut io = VfsJournalStore::new();
        let _ = io.probe_access();
    }
}

#[cfg(test)]
pub(crate) fn load_for_tests(io: &mut impl StoreIo, key: &JournalKey) -> JournalState {
    JournalState::load(io, key).unwrap()
}

#[cfg(test)]
pub(crate) fn persist_placeholder_for_tests(
    state: &mut JournalState,
    io: &mut impl StoreIo,
    key: &JournalKey,
    policy_epoch: u64,
) -> Result<(), backend::StoreError> {
    state.persist_placeholder(io, key, policy_epoch).map(|_| ())
}
