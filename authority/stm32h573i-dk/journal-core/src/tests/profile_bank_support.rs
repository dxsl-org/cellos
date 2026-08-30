use crate::*;
use authority_protocol::Bounded;
use sha2::{Digest, Sha256};
use std::vec::Vec;

#[derive(Clone, Copy)]
pub struct BankAuth;
impl ProfileBankAuthenticator for BankAuth {
    fn authenticate(&self, message: &[u8]) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"test profile bank key");
        hash.update(message);
        hash.finalize().into()
    }
}
#[derive(Clone)]

pub struct BankStorage {
    pub headers: [Vec<u8>; 2],
    pub chunks: [[Vec<u8>; PROFILE_CHUNK_REGIONS]; 2],
    pub erased_chunks: Vec<(u8, u8)>,
    pub sealed: bool,
}

impl BankStorage {
    pub fn empty() -> Self {
        Self {
            headers: core::array::from_fn(|_| Vec::new()),
            chunks: core::array::from_fn(|_| core::array::from_fn(|_| Vec::new())),
            erased_chunks: Vec::new(),
            sealed: false,
        }
    }
}

impl ProfileBankStorage for BankStorage {
    fn read_header(&mut self, slot: u8, output: &mut [u8]) -> Result<usize, BankStorageError> {
        copy(&self.headers[slot as usize], output)
    }

    fn erase_bank(&mut self, slot: u8) -> Result<(), BankStorageError> {
        self.headers[slot as usize].clear();
        for region in &mut self.chunks[slot as usize] {
            region.clear();
        }
        Ok(())
    }

    fn write_header(&mut self, slot: u8, bytes: &[u8]) -> Result<(), BankStorageError> {
        self.headers[slot as usize] = bytes.to_vec();
        Ok(())
    }

    fn read_chunk(
        &mut self,
        slot: u8,
        index: u8,
        output: &mut [u8],
    ) -> Result<usize, BankStorageError> {
        copy(&self.chunks[slot as usize][index as usize], output)
    }

    fn erase_chunk(&mut self, slot: u8, index: u8) -> Result<(), BankStorageError> {
        self.chunks[slot as usize][index as usize].clear();
        self.erased_chunks.push((slot, index));
        Ok(())
    }

    fn write_chunk(&mut self, slot: u8, index: u8, bytes: &[u8]) -> Result<(), BankStorageError> {
        self.chunks[slot as usize][index as usize] = bytes.to_vec();
        Ok(())
    }

    fn seal(&mut self) {
        self.sealed = true;
    }
}

fn copy(value: &[u8], output: &mut [u8]) -> Result<usize, BankStorageError> {
    if value.len() > output.len() {
        return Ok(value.len());
    }
    output[..value.len()].copy_from_slice(value);
    Ok(value.len())
}

pub fn metadata(bytes: &[u8]) -> ProfileBankMetadata {
    ProfileBankMetadata {
        slot: 1,
        device_id: [1; 32],
        authority_id: [2; 32],
        authority_epoch: 3,
        boot_epoch: 4,
        generation: 5,
        policy_epoch: 6,
        upload_handle: 7,
        profile_len: bytes.len() as u32,
        pending_spki_digest: Sha256::digest(b"profile-spki").into(),
        profile_digest: Sha256::digest(bytes).into(),
        spki: Bounded::from_slice(b"profile-spki").unwrap(),
        tpm_public_digest: [8; 32],
    }
}

pub fn initialized(bytes: &[u8]) -> (ProfileBank<BankStorage, BankAuth>, ProfileBankMetadata) {
    let metadata = metadata(bytes);
    let mut bank = ProfileBank::new(BankStorage::empty(), BankAuth);
    bank.initialize(&metadata).unwrap();
    (bank, metadata)
}
