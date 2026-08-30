use super::codec::{decode_chunk, decode_header, metadata_digest};
use super::recovery::exact_chunk;
use super::{
    BankError, ProfileBank, ProfileBankAuthenticator, ProfileBankMetadata, ProfileBankReference,
    ProfileBankStorage, UploadHead, CHUNK_RECORD_MAX, HEADER_MAX,
};
use authority_protocol::{constant_time_eq, PROFILE_MAX_LEN};
use sha2::{Digest, Sha256};

impl<S: ProfileBankStorage, A: ProfileBankAuthenticator> ProfileBank<S, A> {
    /// Authenticate every exact chunk and hash the raw concatenated chain.
    pub fn complete(
        &mut self,
        metadata: &ProfileBankMetadata,
        next_index: u8,
    ) -> Result<ProfileBankReference, BankError> {
        metadata.validate()?;
        if next_index as usize != metadata.chunk_count() {
            return Err(BankError::InvalidSequence);
        }
        if self.recover_upload(metadata, next_index)? != UploadHead::Complete {
            return Err(self.seal());
        }
        let expected_metadata = metadata_digest(metadata);
        let mut hash = Sha256::new();
        for index in 0..metadata.chunk_count() {
            let mut encoded = [0u8; CHUNK_RECORD_MAX];
            let length = self
                .storage
                .read_chunk(metadata.slot, index as u8, &mut encoded)
                .map_err(|_| BankError::Storage)?;
            if length > encoded.len() {
                return Err(self.seal());
            }
            let chunk =
                decode_chunk(&encoded[..length], &self.authenticator).map_err(|_| self.seal())?;
            if !exact_chunk(&chunk, metadata, &expected_metadata, index) {
                return Err(self.seal());
            }
            hash.update(chunk.bytes);
        }
        let actual: [u8; 32] = hash.finalize().into();
        if !constant_time_eq(&actual, &metadata.profile_digest) {
            return Err(self.seal());
        }
        Ok(metadata.reference())
    }

    /// Authenticate the bank named by a recovered journal reference.
    pub fn validate_reference(
        &mut self,
        reference: &ProfileBankReference,
    ) -> Result<(), BankError> {
        if reference.slot > 1
            || reference.generation == 0
            || reference.profile_len == 0
            || reference.profile_len as usize > PROFILE_MAX_LEN
            || reference.spki.is_empty()
        {
            return Err(self.seal());
        }
        let mut header = [0u8; HEADER_MAX];
        let length = self
            .storage
            .read_header(reference.slot, &mut header)
            .map_err(|_| BankError::Storage)?;
        if length > header.len() {
            return Err(self.seal());
        }
        let metadata =
            decode_header(&header[..length], &self.authenticator).ok_or_else(|| self.seal())?;
        if metadata.reference() != *reference {
            return Err(self.seal());
        }
        let count = metadata.chunk_count() as u8;
        let found = self.complete(&metadata, count)?;
        if found != *reference {
            return Err(self.seal());
        }
        Ok(())
    }
}
