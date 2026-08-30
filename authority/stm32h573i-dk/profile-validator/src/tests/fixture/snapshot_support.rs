use authority_protocol::*;
use core::cell::Cell;
use sha2::{Digest, Sha256};
use std::{array, vec::Vec};
use stm32_authority_journal::*;

pub(super) struct Store<'a> {
    pub(super) revision: &'a Cell<u64>,
    pub(super) record: &'a Cell<Option<ProtectedAuthorityRecord>>,
}
impl ProtectedStore for Store<'_> {
    fn compare_and_swap(&mut self, expected: u64, next: &ProtectedAuthorityRecord) -> bool {
        if self.revision.get() != expected || next.revision() != expected + 1 {
            return false;
        }
        self.revision.set(next.revision());
        self.record.set(Some(*next));
        true
    }
    fn seal_on_conflict(&mut self, _: u64) {
        self.record.set(None);
    }
}

pub(super) struct Requests;
impl RequestAuthenticator for Requests {
    fn verify(&self, _: &[u8; REQUEST_AUTH_INPUT_LEN], _: &[u8; 32]) -> bool {
        true
    }
}
pub(super) struct Boot;
impl BootMeasurementVerifier for Boot {
    fn verify_boot_measurement(&self, _: &[u8; 32]) -> bool {
        true
    }
}
pub(super) struct SignedTime;
impl SignedTimeVerifier for SignedTime {
    fn verify_signed_time(&self, _: &AcceptSignedTimeRequest) -> bool {
        true
    }
}
pub(super) struct Clock;
impl TrustedClock for Clock {
    fn now_unix_seconds(&self) -> u64 {
        101
    }
}
pub(super) struct Challenges;
impl TimeChallengeSource for Challenges {
    fn generate_challenge(&mut self) -> Result<([u8; 16], [u8; 32]), AuthorityFault> {
        Ok(([4; 16], [5; 32]))
    }
}

#[derive(Clone, Copy)]
pub(super) struct Auth;
impl RecordAuthenticator for Auth {
    fn authenticate(&self, bytes: &[u8]) -> [u8; 32] {
        tagged_hash(b"journal", bytes)
    }
}
impl ProfileBankAuthenticator for Auth {
    fn authenticate(&self, bytes: &[u8]) -> [u8; 32] {
        tagged_hash(b"bank", bytes)
    }
}
fn tagged_hash(tag: &[u8], bytes: &[u8]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(tag);
    hash.update(bytes);
    hash.finalize().into()
}

pub(super) struct MemoryBank {
    headers: [Vec<u8>; 2],
    chunks: [[Vec<u8>; PROFILE_CHUNK_REGIONS]; 2],
    sealed: bool,
}
impl Default for MemoryBank {
    fn default() -> Self {
        Self {
            headers: array::from_fn(|_| Vec::new()),
            chunks: array::from_fn(|_| array::from_fn(|_| Vec::new())),
            sealed: false,
        }
    }
}
impl ProfileBankStorage for MemoryBank {
    fn read_header(&mut self, slot: u8, out: &mut [u8]) -> Result<usize, BankStorageError> {
        copy(&self.headers[slot as usize], out)
    }
    fn erase_bank(&mut self, slot: u8) -> Result<(), BankStorageError> {
        self.headers[slot as usize].clear();
        for value in &mut self.chunks[slot as usize] {
            value.clear();
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
        out: &mut [u8],
    ) -> Result<usize, BankStorageError> {
        copy(&self.chunks[slot as usize][index as usize], out)
    }
    fn erase_chunk(&mut self, slot: u8, index: u8) -> Result<(), BankStorageError> {
        self.chunks[slot as usize][index as usize].clear();
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
fn copy(value: &[u8], out: &mut [u8]) -> Result<usize, BankStorageError> {
    if value.len() > out.len() {
        return Err(BankStorageError::Unavailable);
    }
    out[..value.len()].copy_from_slice(value);
    Ok(value.len())
}
