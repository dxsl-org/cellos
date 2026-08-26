mod auth;
mod codec;
mod invariants;
pub use auth::PROTECTED_RECORD_MAX;

use super::*;
use crate::AuthorityFault;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtectedAuthorityRecord {
    revision: u64,
    mode: AuthorityMode,
    boot: BootState,
    time: TimeState,
    relay: RelayProfileState,
    device_id: [u8; ID_LEN],
    authority_id: [u8; ID_LEN],
    authority_epoch: u64,
    boot_floor: u64,
    generation_floor: u64,
    state_epoch: u64,
    approved_loader_digest: [u8; DIGEST_LEN],
    last_request_sequence: u64,
    previous_active: Option<RelayIntent>,
    pending_time: Option<PendingTimeChallenge>,
    time_floors: ProtectedTimeFloors,
}

pub trait ProtectedStore {
    fn compare_and_swap(&mut self, expected_revision: u64, next: &ProtectedAuthorityRecord)
        -> bool;

    /// Irreversibly seal the external counter domain before returning.
    fn seal_on_conflict(&mut self, expected_revision: u64);
}

pub trait ProtectedRecordVerifier {
    fn verify(&self, record: &ProtectedAuthorityRecord) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedProtectedRecord(ProtectedAuthorityRecord);

pub fn verify_protected_record<V: ProtectedRecordVerifier>(
    record: ProtectedAuthorityRecord,
    verifier: &V,
) -> Result<VerifiedProtectedRecord, AuthorityFault> {
    if !record.invariants_hold() || !verifier.verify(&record) {
        return Err(AuthorityFault::PersistenceFailure);
    }
    Ok(VerifiedProtectedRecord(record))
}

impl ProtectedAuthorityRecord {
    pub const fn revision(&self) -> u64 {
        self.revision
    }
}

impl VerifiedProtectedRecord {
    pub(super) const fn record(&self) -> &ProtectedAuthorityRecord {
        &self.0
    }
}

impl<S: ProtectedStore> AuthorityState<S> {
    pub fn restore(
        store: S,
        verified: &VerifiedProtectedRecord,
        boot_challenge: [u8; DIGEST_LEN],
    ) -> Self {
        let record = *verified.record();
        Self {
            mode: if record.mode == AuthorityMode::Sealed {
                AuthorityMode::Sealed
            } else {
                AuthorityMode::Ready
            },
            boot: BootState::Closed,
            time: TimeState::Unavailable,
            relay: record.relay,
            device_id: record.device_id,
            authority_id: record.authority_id,
            authority_epoch: record.authority_epoch,
            boot_challenge,
            boot_floor: record.boot_floor,
            generation_floor: record.generation_floor,
            state_epoch: record.state_epoch,
            approved_loader_digest: record.approved_loader_digest,
            last_request_sequence: record.last_request_sequence,
            previous_active: record.previous_active,
            pending_time: None,
            time_floors: record.time_floors,
            protected_revision: record.revision,
            store,
        }
    }
    pub fn into_store(self) -> S {
        self.store
    }

    pub(super) fn persist(&mut self) -> Result<(), AuthorityFault> {
        let expected = self.protected_revision;
        let next_revision = match expected.checked_add(1) {
            Some(value) => value,
            None => {
                self.mode = AuthorityMode::Sealed;
                self.store.seal_on_conflict(expected);
                return Err(AuthorityFault::PersistenceFailure);
            }
        };
        let next = self.snapshot(next_revision);
        if self.store.compare_and_swap(expected, &next) {
            self.protected_revision = next_revision;
            Ok(())
        } else {
            self.mode = AuthorityMode::Sealed;
            self.store.seal_on_conflict(expected);
            Err(AuthorityFault::PersistenceFailure)
        }
    }

    fn snapshot(&self, revision: u64) -> ProtectedAuthorityRecord {
        ProtectedAuthorityRecord {
            revision,
            mode: self.mode,
            boot: self.boot,
            time: self.time,
            relay: self.relay,
            device_id: self.device_id,
            authority_id: self.authority_id,
            authority_epoch: self.authority_epoch,
            boot_floor: self.boot_floor,
            generation_floor: self.generation_floor,
            state_epoch: self.state_epoch,
            approved_loader_digest: self.approved_loader_digest,
            last_request_sequence: self.last_request_sequence,
            previous_active: self.previous_active,
            pending_time: self.pending_time,
            time_floors: self.time_floors,
        }
    }
}
