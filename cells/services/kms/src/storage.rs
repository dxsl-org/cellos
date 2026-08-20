#![cfg_attr(not(test), allow(dead_code, unused_imports))]

mod backend;
mod journal;
mod provider;
mod ready;
mod record;
mod root;
#[cfg(target_os = "none")]
mod runtime;

pub(crate) use backend::{StoreError, StoreIo};
pub(crate) use journal::{JournalLoad, JournalState};
pub(crate) use provider::{
    OpenedRoot, ProviderAssessment, ProviderOpenResult, RootProvider, UnavailableRootProvider,
};
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

pub(crate) fn runtime_root() -> RootAssessment {
    RootAssessment::from_provider(&UnavailableRootProvider, &JournalState::empty())
}

#[cfg(test)]
pub(crate) use journal::ActiveSlot;

#[cfg(test)]
pub(crate) use record::authenticator;

#[cfg(test)]
pub(crate) use provider::FixtureRootProvider;

#[cfg(test)]
pub(crate) fn assess_for_tests(
    provider: &impl RootProvider,
    journal: &JournalState,
) -> RootAssessment {
    RootAssessment::from_provider(provider, journal)
}
