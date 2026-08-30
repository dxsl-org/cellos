use crate::model::material::matches_upload_key;
use crate::{RecoveredRecord, SPKI_MAX};
use authority_protocol::{Bounded, RelayProfileState, DIGEST_LEN};

/// Bank-gated immutable inputs for closed profile validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingEnrollmentSnapshot {
    journal_revision: u64,
    protected_revision: u64,
    csr_handle: u64,
    device_id: [u8; DIGEST_LEN],
    authority_id: [u8; DIGEST_LEN],
    authority_epoch: u64,
    boot_epoch: u64,
    generation: u64,
    policy_epoch: u64,
    upload_handle: u64,
    pending_slot: u8,
    spki: Bounded<SPKI_MAX>,
    spki_digest: [u8; DIGEST_LEN],
    profile_len: u32,
    profile_digest: [u8; DIGEST_LEN],
    tpm_public_digest: [u8; DIGEST_LEN],
}

impl RecoveredRecord {
    pub fn pending_enrollment_snapshot(&self) -> Option<PendingEnrollmentSnapshot> {
        let bindings = self.record.protected.bindings();
        let RelayProfileState::Uploading(intent) = bindings.relay else {
            return None;
        };
        let material = self.record.pending.as_ref()?;
        if !self.upload_bank_complete
            || self.record.counter != bindings.revision
            || !matches_upload_key(material, intent)
        {
            return None;
        }
        Some(PendingEnrollmentSnapshot {
            journal_revision: self.record.counter,
            protected_revision: bindings.revision,
            csr_handle: intent.csr_handle,
            device_id: intent.device_id,
            authority_id: intent.authority_id,
            authority_epoch: intent.authority_epoch,
            boot_epoch: intent.boot_epoch,
            generation: intent.generation,
            policy_epoch: intent.policy_epoch,
            upload_handle: intent.upload_handle,
            pending_slot: intent.pending_slot,
            spki: material.spki,
            spki_digest: intent.pending_spki_digest,
            profile_len: intent.profile_len,
            profile_digest: intent.profile_digest,
            tpm_public_digest: intent.tpm_public_digest,
        })
    }
}

impl PendingEnrollmentSnapshot {
    pub const fn journal_revision(&self) -> u64 {
        self.journal_revision
    }
    pub const fn protected_revision(&self) -> u64 {
        self.protected_revision
    }
    pub const fn csr_handle(&self) -> u64 {
        self.csr_handle
    }
    pub const fn device_id(&self) -> &[u8; DIGEST_LEN] {
        &self.device_id
    }
    pub const fn authority_id(&self) -> &[u8; DIGEST_LEN] {
        &self.authority_id
    }
    pub const fn authority_epoch(&self) -> u64 {
        self.authority_epoch
    }
    pub const fn boot_epoch(&self) -> u64 {
        self.boot_epoch
    }
    pub const fn generation(&self) -> u64 {
        self.generation
    }
    pub const fn policy_epoch(&self) -> u64 {
        self.policy_epoch
    }
    pub const fn upload_handle(&self) -> u64 {
        self.upload_handle
    }
    pub const fn pending_slot(&self) -> u8 {
        self.pending_slot
    }
    pub fn spki(&self) -> &[u8] {
        self.spki.as_slice()
    }
    pub const fn spki_digest(&self) -> &[u8; DIGEST_LEN] {
        &self.spki_digest
    }
    pub const fn profile_len(&self) -> u32 {
        self.profile_len
    }
    pub const fn profile_digest(&self) -> &[u8; DIGEST_LEN] {
        &self.profile_digest
    }
    pub const fn tpm_public_digest(&self) -> &[u8; DIGEST_LEN] {
        &self.tpm_public_digest
    }
}
