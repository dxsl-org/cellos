use super::snapshot_flow::{context, pending_state, validated};
use super::snapshot_support::{Auth, MemoryBank};
use authority_protocol::*;
use core::cell::Cell;
use sha2::{Digest, Sha256};
use stm32_authority_journal::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum MediaCut {
    None,
    FirstHeaderRead,
    HeaderReadback,
    FirstChunkRead,
    ChunkReadback,
}

pub(super) struct CutStore<'a> {
    pub(super) revision: &'a Cell<u64>,
    pub(super) record: &'a Cell<Option<ProtectedAuthorityRecord>>,
    pub(super) acknowledge_next: &'a Cell<Option<u8>>,
    pub(super) fired: &'a Cell<bool>,
}

impl ProtectedStore for CutStore<'_> {
    fn compare_and_swap(&mut self, expected: u64, next: &ProtectedAuthorityRecord) -> bool {
        if let RelayProfileState::Uploading(upload) = next.bindings().relay {
            if self.acknowledge_next.get() == Some(upload.next_index) {
                self.fired.set(true);
                panic!("power cut before protected acknowledgement");
            }
        }
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

pub(super) struct CuttingBank {
    pub(super) inner: MemoryBank,
    pub(super) cut: MediaCut,
    pub(super) header_reads: u8,
    pub(super) chunk_reads: u8,
    pub(super) fired: bool,
}

impl ProfileBankStorage for CuttingBank {
    fn read_header(&mut self, slot: u8, output: &mut [u8]) -> Result<usize, BankStorageError> {
        self.header_reads += 1;
        if self.cut == MediaCut::FirstHeaderRead && self.header_reads == 1 {
            self.fired = true;
            panic!("power cut before bank initialization");
        }
        let result = self.inner.read_header(slot, output);
        if self.cut == MediaCut::HeaderReadback && self.header_reads == 2 {
            self.fired = true;
            panic!("power cut after header readback");
        }
        result
    }

    fn erase_bank(&mut self, slot: u8) -> Result<(), BankStorageError> {
        self.inner.erase_bank(slot)
    }

    fn write_header(&mut self, slot: u8, bytes: &[u8]) -> Result<(), BankStorageError> {
        self.inner.write_header(slot, bytes)
    }

    fn read_chunk(
        &mut self,
        slot: u8,
        index: u8,
        output: &mut [u8],
    ) -> Result<usize, BankStorageError> {
        self.chunk_reads += 1;
        if self.cut == MediaCut::FirstChunkRead && self.chunk_reads == 1 {
            self.fired = true;
            panic!("power cut before chunk write");
        }
        let result = self.inner.read_chunk(slot, index, output);
        if self.cut == MediaCut::ChunkReadback
            && self.chunk_reads as usize == PROFILE_CHUNK_REGIONS + 1
        {
            self.fired = true;
            panic!("power cut after chunk readback");
        }
        result
    }

    fn erase_chunk(&mut self, slot: u8, index: u8) -> Result<(), BankStorageError> {
        self.inner.erase_chunk(slot, index)
    }

    fn write_chunk(&mut self, slot: u8, index: u8, bytes: &[u8]) -> Result<(), BankStorageError> {
        self.inner.write_chunk(slot, index, bytes)
    }

    fn seal(&mut self) {
        self.inner.seal();
    }
}

struct AcceptRecord;
impl ProtectedRecordVerifier for AcceptRecord {
    fn verify(&self, _: &ProtectedAuthorityRecord) -> bool {
        true
    }
}

pub(super) fn upload_metadata() -> ProfileBankMetadata {
    let spki = Bounded::from_slice(b"spki").unwrap();
    ProfileBankMetadata {
        slot: 0,
        device_id: [1; 32],
        authority_id: [2; 32],
        authority_epoch: 1,
        boot_epoch: 1,
        generation: 1,
        policy_epoch: 3,
        upload_handle: 11,
        profile_len: 4,
        profile_digest: Sha256::digest([9; 4]).into(),
        pending_spki_digest: Sha256::digest(spki.as_slice()).into(),
        spki,
        tpm_public_digest: [8; 32],
    }
}

pub(super) fn begin_request(
    metadata: &ProfileBankMetadata,
    sequence: u64,
) -> ValidatedRequest<BeginRelayProfileUploadRequest> {
    validated(BeginRelayProfileUploadRequest {
        context: context(sequence, 1, Operation::BeginRelayProfileUpload),
        upload_handle: metadata.upload_handle,
        generation: metadata.generation,
        policy_epoch: metadata.policy_epoch,
        pending_slot: metadata.slot,
        pending_spki_digest: metadata.pending_spki_digest,
        profile_digest: metadata.profile_digest,
        tpm_public_digest: metadata.tpm_public_digest,
        profile_len: metadata.profile_len,
    })
}

pub(super) fn chunk_request() -> ValidatedRequest<WriteRelayProfileChunkRequest> {
    validated(WriteRelayProfileChunkRequest {
        context: context(6, 1, Operation::WriteRelayProfileChunk),
        upload_handle: 11,
        chunk_index: 0,
        chunk: Bounded::from_slice(&[9; 4]).unwrap(),
    })
}

pub(super) fn reboot_relay(record: ProtectedAuthorityRecord) -> RelayProfileState {
    let verified = verify_protected_record(record, &AcceptRecord).unwrap();
    let revision = Cell::new(record.revision());
    let saved = Cell::new(Some(record));
    let cut = Cell::new(None);
    let fired = Cell::new(false);
    AuthorityState::restore(
        CutStore {
            revision: &revision,
            record: &saved,
            acknowledge_next: &cut,
            fired: &fired,
        },
        &verified,
        [3; 32],
    )
    .relay_state()
}
