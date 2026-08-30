use super::codec::{decode_chunk, encode_chunk, metadata_digest};
use super::recovery::exact_chunk;
use super::{
    BankError, ProfileBank, ProfileBankAuthenticator, ProfileBankMetadata, ProfileBankStorage,
    UploadHead, CHUNK_RECORD_MAX,
};

impl<S: ProfileBankStorage, A: ProfileBankAuthenticator> ProfileBank<S, A> {
    /// Commit one sequential chunk, or accept one byte-exact authenticated retry.
    pub fn write_chunk(
        &mut self,
        metadata: &ProfileBankMetadata,
        next_index: u8,
        index: u8,
        chunk: &[u8],
    ) -> Result<u8, BankError> {
        metadata.validate()?;
        if index > next_index || index as usize >= metadata.chunk_count() {
            return Err(BankError::InvalidSequence);
        }
        let expected_len = metadata
            .chunk_len(index as usize)
            .ok_or(BankError::InvalidSequence)?;
        if index < next_index && chunk.len() != expected_len {
            return Err(self.seal());
        }
        if index == next_index && chunk.len() != expected_len {
            return Err(BankError::InvalidChunk);
        }
        let head = self.recover_upload(metadata, next_index)?;
        if index < next_index {
            return if self.stored_bytes_equal(metadata, index, chunk)? {
                Ok(next_index)
            } else {
                Err(self.seal())
            };
        }
        match head {
            UploadHead::ExactRetryCandidate => {
                if !self.stored_bytes_equal(metadata, index, chunk)? {
                    return Err(self.seal());
                }
            }
            UploadHead::AwaitingWrite => self.store_current(metadata, index, chunk)?,
            UploadHead::Complete => return Err(BankError::InvalidSequence),
        }
        next_index.checked_add(1).ok_or_else(|| self.seal())
    }

    fn store_current(
        &mut self,
        metadata: &ProfileBankMetadata,
        index: u8,
        chunk: &[u8],
    ) -> Result<(), BankError> {
        self.storage
            .erase_chunk(metadata.slot, index)
            .map_err(|_| BankError::Storage)?;
        let mut encoded = [0u8; CHUNK_RECORD_MAX];
        let length = encode_chunk(metadata, index, chunk, &self.authenticator, &mut encoded)
            .map_err(|_| BankError::InvalidChunk)?;
        self.storage
            .write_chunk(metadata.slot, index, &encoded[..length])
            .map_err(|_| BankError::Storage)?;
        let mut read_back = [0u8; CHUNK_RECORD_MAX];
        let read_len = self
            .storage
            .read_chunk(metadata.slot, index, &mut read_back)
            .map_err(|_| BankError::Storage)?;
        if read_len != length || read_back[..read_len] != encoded[..length] {
            return Err(self.seal());
        }
        Ok(())
    }

    fn stored_bytes_equal(
        &mut self,
        metadata: &ProfileBankMetadata,
        index: u8,
        expected: &[u8],
    ) -> Result<bool, BankError> {
        let mut encoded = [0u8; CHUNK_RECORD_MAX];
        let length = self
            .storage
            .read_chunk(metadata.slot, index, &mut encoded)
            .map_err(|_| BankError::Storage)?;
        if length > encoded.len() {
            return Err(self.seal());
        }
        let decoded =
            decode_chunk(&encoded[..length], &self.authenticator).map_err(|_| self.seal())?;
        let digest = metadata_digest(metadata);
        Ok(exact_chunk(&decoded, metadata, &digest, index as usize) && decoded.bytes == expected)
    }
}
