use super::codec::{decode_chunk, decode_header, metadata_digest, Chunk, ChunkDecodeError};
use super::{
    BankError, ProfileBank, ProfileBankAuthenticator, ProfileBankMetadata, ProfileBankStorage,
    UploadHead, CHUNK_RECORD_MAX, HEADER_MAX, PROFILE_CHUNK_REGIONS,
};

impl<S: ProfileBankStorage, A: ProfileBankAuthenticator> ProfileBank<S, A> {
    /// Authenticate the committed prefix and classify only the current region.
    pub fn recover_upload(
        &mut self,
        metadata: &ProfileBankMetadata,
        next_index: u8,
    ) -> Result<UploadHead, BankError> {
        metadata.validate()?;
        let count = metadata.chunk_count();
        if next_index as usize > count {
            return Err(self.seal());
        }
        self.require_header(metadata)?;
        let expected_digest = metadata_digest(metadata);
        let mut head = if next_index as usize == count {
            UploadHead::Complete
        } else {
            UploadHead::AwaitingWrite
        };
        for index in 0..PROFILE_CHUNK_REGIONS {
            let mut encoded = [0u8; CHUNK_RECORD_MAX];
            let length = self
                .storage
                .read_chunk(metadata.slot, index as u8, &mut encoded)
                .map_err(|_| BankError::Storage)?;
            if length > encoded.len() {
                return Err(self.seal());
            }
            let decoded = decode_chunk(&encoded[..length], &self.authenticator);
            if index < next_index as usize {
                let chunk = decoded.map_err(|_| self.seal())?;
                if !exact_chunk(&chunk, metadata, &expected_digest, index) {
                    return Err(self.seal());
                }
            } else if index == next_index as usize && index < count {
                match decoded {
                    Ok(chunk) if exact_chunk(&chunk, metadata, &expected_digest, index) => {
                        head = UploadHead::ExactRetryCandidate;
                    }
                    Ok(_) | Err(ChunkDecodeError::Malformed) => return Err(self.seal()),
                    Err(ChunkDecodeError::Unauthenticated) => {}
                }
            } else {
                match decoded {
                    Ok(_) | Err(ChunkDecodeError::Malformed) => return Err(self.seal()),
                    Err(ChunkDecodeError::Unauthenticated) => {}
                }
            }
        }
        Ok(head)
    }

    pub(crate) fn require_header(
        &mut self,
        metadata: &ProfileBankMetadata,
    ) -> Result<(), BankError> {
        let mut encoded = [0u8; HEADER_MAX];
        let length = self
            .storage
            .read_header(metadata.slot, &mut encoded)
            .map_err(|_| BankError::Storage)?;
        if length > encoded.len() {
            return Err(self.seal());
        }
        match decode_header(&encoded[..length], &self.authenticator) {
            Some(found) if found == *metadata => Ok(()),
            _ => Err(self.seal()),
        }
    }
}

pub(crate) fn exact_chunk(
    chunk: &Chunk<'_>,
    metadata: &ProfileBankMetadata,
    expected_digest: &[u8; 32],
    index: usize,
) -> bool {
    chunk.slot == metadata.slot
        && chunk.index as usize == index
        && chunk.metadata_digest == *expected_digest
        && metadata.chunk_len(index) == Some(chunk.bytes.len())
}
