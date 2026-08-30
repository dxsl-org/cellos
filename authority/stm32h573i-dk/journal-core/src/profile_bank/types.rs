use crate::{ProfileMaterial, SPKI_MAX};
use authority_protocol::{constant_time_eq, Bounded, DIGEST_LEN, PROFILE_MAX_LEN};
pub use authority_protocol::{
    PROFILE_CHUNK_MAX as PROFILE_CHUNK_SIZE, PROFILE_MAX_CHUNKS as PROFILE_CHUNK_REGIONS,
};
use sha2::{Digest, Sha256};
pub const PROFILE_BANK_HEADER_MAX: usize = 352;
pub const PROFILE_BANK_CHUNK_REGION_MAX: usize = 864;
pub(crate) const HEADER_MAX: usize = PROFILE_BANK_HEADER_MAX;
pub(crate) const CHUNK_RECORD_MAX: usize = PROFILE_BANK_CHUNK_REGION_MAX;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileBankMetadata {
    pub slot: u8,
    pub device_id: [u8; DIGEST_LEN],
    pub authority_id: [u8; DIGEST_LEN],
    pub authority_epoch: u64,
    pub boot_epoch: u64,
    pub generation: u64,
    pub policy_epoch: u64,
    pub upload_handle: u64,
    pub profile_len: u32,
    pub profile_digest: [u8; DIGEST_LEN],
    pub pending_spki_digest: [u8; DIGEST_LEN],
    pub spki: Bounded<SPKI_MAX>,
    pub tpm_public_digest: [u8; DIGEST_LEN],
}

impl ProfileBankMetadata {
    pub(crate) fn validate(&self) -> Result<(), BankError> {
        let length = self.profile_len as usize;
        if self.slot > 1
            || self.generation == 0
            || self.upload_handle == 0
            || length == 0
            || length > PROFILE_MAX_LEN
            || self.spki.is_empty()
            || !constant_time_eq(
                &Sha256::digest(self.spki.as_slice()),
                &self.pending_spki_digest,
            )
        {
            return Err(BankError::InvalidMetadata);
        }
        Ok(())
    }

    pub(crate) fn chunk_count(&self) -> usize {
        (self.profile_len as usize).div_ceil(PROFILE_CHUNK_SIZE)
    }

    pub(crate) fn chunk_len(&self, index: usize) -> Option<usize> {
        if index >= self.chunk_count() {
            return None;
        }
        let consumed = index * PROFILE_CHUNK_SIZE;
        Some((self.profile_len as usize - consumed).min(PROFILE_CHUNK_SIZE))
    }

    pub(crate) fn reference(&self) -> ProfileBankReference {
        ProfileMaterial {
            device_id: self.device_id,
            authority_id: self.authority_id,
            authority_epoch: self.authority_epoch,
            boot_epoch: self.boot_epoch,
            slot: self.slot,
            generation: self.generation,
            profile_len: self.profile_len,
            profile_digest: self.profile_digest,
            tpm_public_digest: self.tpm_public_digest,
            spki: self.spki,
        }
    }
}

pub type ProfileBankReference = ProfileMaterial;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadHead {
    AwaitingWrite,
    ExactRetryCandidate,
    Complete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BankStorageError {
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BankError {
    Storage,
    InvalidMetadata,
    InvalidChunk,
    InvalidSequence,
    Sealed,
}

/// Fixed physical profile-bank regions supplied by the firmware adapter.
pub trait ProfileBankStorage {
    fn read_header(&mut self, slot: u8, output: &mut [u8]) -> Result<usize, BankStorageError>;
    fn erase_bank(&mut self, slot: u8) -> Result<(), BankStorageError>;
    fn write_header(&mut self, slot: u8, bytes: &[u8]) -> Result<(), BankStorageError>;
    fn read_chunk(
        &mut self,
        slot: u8,
        index: u8,
        output: &mut [u8],
    ) -> Result<usize, BankStorageError>;
    fn erase_chunk(&mut self, slot: u8, index: u8) -> Result<(), BankStorageError>;
    fn write_chunk(&mut self, slot: u8, index: u8, bytes: &[u8]) -> Result<(), BankStorageError>;
    /// Irreversibly prevent this authority domain from serving.
    fn seal(&mut self);
}

/// Produces fixed tags under the device-bound profile-bank key.
pub trait ProfileBankAuthenticator {
    fn authenticate(&self, message: &[u8]) -> [u8; DIGEST_LEN];
}
