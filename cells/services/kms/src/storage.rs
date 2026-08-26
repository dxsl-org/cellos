#![cfg_attr(not(test), allow(dead_code, unused_imports))]

mod backend;
mod capability;
mod enroll;
mod journal;
mod provider;
mod ready;
mod record;
#[cfg(test)]
mod relay_fixture;
mod root;
#[cfg(target_os = "none")]
mod runtime;
mod tls;

pub(crate) use backend::{StoreError, StoreIo};
pub(crate) use capability::{
    C2cX25519Status, EnrollmentKeyDestroyConfirmation, RelayP256Status, RelaySignError,
};
pub(crate) use enroll::verify_and_assemble_csr;
pub(crate) use journal::{JournalLoad, JournalState};
pub(crate) use provider::{
    C2cProvider, OpenedRoot, ProviderAssessment, ProviderOpenResult, ProviderSlot,
};
pub(crate) use record::{JournalKey, JournalRecord, SlotId, SLOT_A_PATH, SLOT_B_PATH, STORE_DIR};
pub(crate) use root::RootAssessment;
pub(crate) use tls::normalize_and_verify_tls13_signature;

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

#[cfg(test)]
pub(crate) fn persist_relay_state_for_tests(
    state: &mut JournalState,
    io: &mut impl StoreIo,
    key: &JournalKey,
    protected: crate::lifecycle::ProtectedRelayState,
) -> Result<(), backend::StoreError> {
    state.persist_relay_state(io, key, protected).map(|_| ())
}

#[cfg(test)]
pub(crate) fn protected_relay_state_for_tests(
    state: &JournalState,
) -> Option<crate::lifecycle::ProtectedRelayState> {
    state.protected_relay_state()
}

/// Load authenticated relay lifecycle facts plus a strictly newer protected
/// restart epoch. No runtime sealing key/monotonic provider is wired yet, so
/// the honest result is unavailable and dispatch seals the service.
#[cfg(not(test))]
pub(crate) fn load_runtime_protected_relay_state(
) -> Result<(u64, crate::lifecycle::ProtectedRelayState), StoreError> {
    Err(StoreError::PermissionDenied)
}

/// Persist authenticated lifecycle facts. Production remains blocked until a
/// provider can authenticate the journal and advance its monotonic epoch.
#[cfg(not(test))]
pub(crate) fn persist_runtime_protected_relay_state(
    _protected: crate::lifecycle::ProtectedRelayState,
) -> Result<(), StoreError> {
    Err(StoreError::PermissionDenied)
}

pub(crate) fn runtime_root(provider: &ProviderSlot) -> RootAssessment {
    RootAssessment::from_provider(provider, &JournalState::empty())
}

#[cfg(test)]
pub(crate) use journal::ActiveSlot;

#[cfg(test)]
pub(crate) use record::authenticator;

#[cfg(test)]
pub(crate) use provider::FixtureRootProvider;
#[cfg(test)]
pub(crate) use relay_fixture::{
    FixtureRelayProvider, FixtureSignatureBehavior, FIXTURE_PROFILE_DIGEST,
    FIXTURE_RELAY_GENERATION,
};

#[cfg(test)]
pub(crate) fn assess_for_tests(
    provider: &impl C2cProvider,
    journal: &JournalState,
) -> RootAssessment {
    RootAssessment::from_provider(provider, journal)
}
