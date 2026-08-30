mod codec;
mod recovery;
mod reference;
mod service;
mod types;
mod write;

pub use types::{
    BankError, BankStorageError, ProfileBankAuthenticator, ProfileBankMetadata,
    ProfileBankReference, ProfileBankStorage, UploadHead, PROFILE_BANK_CHUNK_REGION_MAX,
    PROFILE_BANK_HEADER_MAX, PROFILE_CHUNK_REGIONS, PROFILE_CHUNK_SIZE,
};

use codec::{decode_header, encode_header};
use types::{CHUNK_RECORD_MAX, HEADER_MAX};

/// Allocation-free authenticated access to two external profile banks.
pub struct ProfileBank<S, A> {
    storage: S,
    authenticator: A,
}

impl<S: ProfileBankStorage, A: ProfileBankAuthenticator> ProfileBank<S, A> {
    pub const fn new(storage: S, authenticator: A) -> Self {
        Self {
            storage,
            authenticator,
        }
    }

    pub fn into_parts(self) -> (S, A) {
        (self.storage, self.authenticator)
    }

    /// Initialize and read back an inactive physical bank.
    pub fn initialize(&mut self, metadata: &ProfileBankMetadata) -> Result<(), BankError> {
        metadata.validate()?;
        let mut current = [0u8; HEADER_MAX];
        let current_len = self
            .storage
            .read_header(metadata.slot, &mut current)
            .map_err(|_| BankError::Storage)?;
        if current_len > current.len() {
            return Err(self.seal());
        }
        if decode_header(&current[..current_len], &self.authenticator).as_ref() == Some(metadata) {
            return Ok(());
        }
        self.storage
            .erase_bank(metadata.slot)
            .map_err(|_| BankError::Storage)?;
        let mut encoded = [0u8; HEADER_MAX];
        let length = encode_header(metadata, &self.authenticator, &mut encoded)
            .map_err(|_| BankError::InvalidMetadata)?;
        self.storage
            .write_header(metadata.slot, &encoded[..length])
            .map_err(|_| BankError::Storage)?;
        let read_len = self
            .storage
            .read_header(metadata.slot, &mut current)
            .map_err(|_| BankError::Storage)?;
        if read_len != length || current[..read_len] != encoded[..length] {
            return Err(self.seal());
        }
        Ok(())
    }

    pub(crate) fn seal(&mut self) -> BankError {
        self.storage.seal();
        BankError::Sealed
    }
}
