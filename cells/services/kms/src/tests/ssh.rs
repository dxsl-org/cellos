//! KSSH K5 host tests: custody behaviour of opcodes 15-24 (feature
//! `ssh-dev-custody`). Shared drivers live here; the refusal-matrix rows of
//! `k5-cp1.md` section 13.3 are in the child modules.

use core::cell::Cell;
use std::format;
use std::string::String;
use std::vec::Vec;

use api::caller_identity::{CALLER_FLAG_KMS_SSH_ADMIN, CALLER_FLAG_KMS_SSH_SIGN};
use sha2::{Digest, Sha256};
use types::kms::{
    ssh_chunk_count, KmsCapabilityReadiness, KmsCustodyPosture, KmsKeyAlgorithm, KmsOpcode,
    KmsProviderKind, KmsRequestV1, KmsSshKeyCustody, SshAbortRequestPayload, SshChunkAckPayload,
    SshChunkRequestPayload, SshDeleteRequestPayload, SshImportBeginRequestPayload,
    SshImportCommitRequestPayload, SshKeyStatusPayload, SshSignRequestPayload,
    SshSignResponsePayload, SshStatusRequestPayload, SshTransferBeginResponsePayload,
    SshUnlockBeginRequestPayload, KMS_SSH_CHUNK_LEN,
};

use super::*;
use crate::ssh::{NoPersist, SshLeaf, SshPersist};

mod admission;
mod fixtures;
mod import;
mod kdf_cost;
mod mutation;
mod parser;
#[cfg(feature = "ssh-dev-persist")]
mod persist;
mod stack;
mod store;
mod unlock_sign;
mod vectors;
mod zeroize;

/// A synthetic attested caller. The kernel is not involved: `flags` is a plain
/// field of `CallerIdentity`.
pub(super) struct Actor {
    pub cell: u64,
    pub generation: u64,
    pub tid: usize,
    pub flags: u32,
    next_id: Cell<u64>,
}

impl Actor {
    pub(super) fn new(cell: u64, generation: u64, tid: usize, flags: u32) -> Self {
        Self {
            cell,
            generation,
            tid,
            flags,
            next_id: Cell::new(1),
        }
    }

    pub(super) fn admin() -> Self {
        Self::new(100, 1, 10, CALLER_FLAG_KMS_SSH_ADMIN)
    }

    pub(super) fn login() -> Self {
        Self::new(200, 3, 20, CALLER_FLAG_KMS_SSH_SIGN)
    }

    pub(super) fn identity(&self) -> Option<CallerIdentity> {
        Some(CallerIdentity {
            flags: self.flags,
            cell_id: self.cell,
            generation: self.generation,
            sender_tid: self.tid as u64,
        })
    }

    pub(super) fn principal(&self) -> crate::ssh::Principal {
        crate::ssh::Principal {
            cell_id: self.cell,
            generation: self.generation,
        }
    }

    /// A fresh, strictly increasing request id.
    pub(super) fn rid(&self) -> u64 {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        id
    }

    pub(super) fn set_next(&self, id: u64) {
        self.next_id.set(id);
    }
}

/// Device under test: the whole `KmsService` (frames) or a bare leaf with a
/// chosen persister.
pub(super) trait Dut {
    type P: SshPersist;
    fn leaf(&mut self) -> &mut SshLeaf<Self::P>;
    fn raw(
        &mut self,
        opcode: KmsOpcode,
        payload: &[u8],
        actor: &Actor,
    ) -> Result<Vec<u8>, KmsErrorCode>;
}

impl Dut for KmsService {
    type P = NoPersist;

    fn leaf(&mut self) -> &mut SshLeaf<NoPersist> {
        &mut self.ssh
    }

    fn raw(
        &mut self,
        opcode: KmsOpcode,
        payload: &[u8],
        actor: &Actor,
    ) -> Result<Vec<u8>, KmsErrorCode> {
        let frame = KmsRequestV1::new(opcode, 41, payload).unwrap().to_bytes();
        let response = self
            .handle(&frame, actor.tid, actor.identity(), registry(None, Some(8)))
            .expect("canonical frame is answered");
        assert_eq!(response.opcode().unwrap(), opcode);
        assert_eq!(response.request_id, 41);
        match response.error_code().unwrap() {
            Some(code) => {
                assert!(response.payload().unwrap().is_empty());
                Err(code)
            }
            None => Ok(response.payload().unwrap().to_vec()),
        }
    }
}

impl<P: SshPersist> Dut for SshLeaf<P> {
    type P = P;

    fn leaf(&mut self) -> &mut SshLeaf<P> {
        self
    }

    fn raw(
        &mut self,
        opcode: KmsOpcode,
        payload: &[u8],
        actor: &Actor,
    ) -> Result<Vec<u8>, KmsErrorCode> {
        self.handle(opcode, payload, actor.tid, actor.identity())
            .map(|payload| payload.as_slice().to_vec())
    }
}

pub(super) fn sha(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

pub(super) fn name_field(name: &[u8]) -> [u8; 64] {
    let mut field = [0u8; 64];
    field[..name.len()].copy_from_slice(name);
    field
}

pub(super) fn chunk_payload(handle: u64, index: u32, data: &[u8]) -> [u8; 112] {
    let mut chunk = [0u8; KMS_SSH_CHUNK_LEN];
    chunk[..data.len()].copy_from_slice(data);
    SshChunkRequestPayload {
        handle,
        chunk_index: index,
        chunk_len: data.len() as u16,
        reserved: 0,
        chunk,
    }
    .encode()
}

pub(super) fn begin_import<D: Dut>(
    dut: &mut D,
    actor: &Actor,
    name: &[u8],
    total_len: u32,
    digest: [u8; 32],
) -> Result<SshTransferBeginResponsePayload, KmsErrorCode> {
    let request = SshImportBeginRequestPayload {
        request_id: actor.rid(),
        total_len,
        name_len: name.len() as u8,
        reserved: [0; 3],
        blob_sha256: digest,
        name: name_field(name),
    };
    dut.raw(KmsOpcode::BeginSshKeyImport, &request.encode(), actor)
        .map(|bytes| SshTransferBeginResponsePayload::decode(&bytes).unwrap())
}

pub(super) fn import_chunk<D: Dut>(
    dut: &mut D,
    actor: &Actor,
    handle: u64,
    index: u32,
    data: &[u8],
) -> Result<SshChunkAckPayload, KmsErrorCode> {
    dut.raw(
        KmsOpcode::ImportSshKeyChunk,
        &chunk_payload(handle, index, data),
        actor,
    )
    .map(|bytes| SshChunkAckPayload::decode(&bytes).unwrap())
}

pub(super) fn commit_import<D: Dut>(
    dut: &mut D,
    actor: &Actor,
    handle: u64,
    chunk_count: u32,
    digest: [u8; 32],
) -> Result<SshKeyStatusPayload, KmsErrorCode> {
    let request = SshImportCommitRequestPayload {
        handle,
        chunk_count,
        reserved: 0,
        blob_sha256: digest,
    };
    dut.raw(KmsOpcode::CommitSshKeyImport, &request.encode(), actor)
        .map(|bytes| SshKeyStatusPayload::decode(&bytes).unwrap())
}

pub(super) fn abort_import<D: Dut>(
    dut: &mut D,
    actor: &Actor,
    handle: u64,
) -> Result<(), KmsErrorCode> {
    dut.raw(
        KmsOpcode::AbortSshKeyImport,
        &SshAbortRequestPayload { handle }.encode(),
        actor,
    )
    .map(|bytes| assert!(bytes.is_empty()))
}

/// Begin, every canonical chunk, then Commit.
pub(super) fn import_blob<D: Dut>(
    dut: &mut D,
    actor: &Actor,
    name: &[u8],
    blob: &[u8],
) -> Result<SshKeyStatusPayload, KmsErrorCode> {
    let digest = sha(blob);
    let begin = begin_import(dut, actor, name, blob.len() as u32, digest)?;
    assert_eq!(begin.chunk_count, ssh_chunk_count(blob.len() as u32));
    for (index, data) in blob.chunks(KMS_SSH_CHUNK_LEN).enumerate() {
        import_chunk(dut, actor, begin.handle, index as u32, data)?;
    }
    commit_import(dut, actor, begin.handle, begin.chunk_count, digest)
}

pub(super) fn status<D: Dut>(
    dut: &mut D,
    actor: &Actor,
    name: &[u8],
) -> Result<SshKeyStatusPayload, KmsErrorCode> {
    let request = SshStatusRequestPayload {
        name_len: name.len() as u8,
        name: name_field(name),
    };
    dut.raw(KmsOpcode::GetSshKeyStatus, &request.encode(), actor)
        .map(|bytes| SshKeyStatusPayload::decode(&bytes).unwrap())
}

pub(super) fn delete_key<D: Dut>(
    dut: &mut D,
    actor: &Actor,
    name: &[u8],
    public_key: [u8; 32],
) -> Result<(), KmsErrorCode> {
    let request = SshDeleteRequestPayload {
        request_id: actor.rid(),
        public_key,
        name_len: name.len() as u8,
        name: name_field(name),
    };
    dut.raw(KmsOpcode::DeleteSshKey, &request.encode(), actor)
        .map(|bytes| assert!(bytes.is_empty()))
}

pub(super) fn unlock_begin<D: Dut>(
    dut: &mut D,
    actor: &Actor,
    public_key: [u8; 32],
    secret_len: u16,
) -> Result<SshTransferBeginResponsePayload, KmsErrorCode> {
    let request = SshUnlockBeginRequestPayload {
        request_id: actor.rid(),
        public_key,
        secret_len,
        reserved0: 0,
        reserved1: 0,
    };
    dut.raw(KmsOpcode::BeginSshUnlock, &request.encode(), actor)
        .map(|bytes| SshTransferBeginResponsePayload::decode(&bytes).unwrap())
}

pub(super) fn unlock_chunk<D: Dut>(
    dut: &mut D,
    actor: &Actor,
    handle: u64,
    index: u32,
    data: &[u8],
) -> Result<SshChunkAckPayload, KmsErrorCode> {
    dut.raw(
        KmsOpcode::SshUnlockChunk,
        &chunk_payload(handle, index, data),
        actor,
    )
    .map(|bytes| SshChunkAckPayload::decode(&bytes).unwrap())
}

pub(super) fn unlock_abort<D: Dut>(
    dut: &mut D,
    actor: &Actor,
    handle: u64,
) -> Result<(), KmsErrorCode> {
    dut.raw(
        KmsOpcode::AbortSshUnlock,
        &SshAbortRequestPayload { handle }.encode(),
        actor,
    )
    .map(|bytes| assert!(bytes.is_empty()))
}

/// Unlock Begin plus every chunk of `secret`: the slot ends `Staged`.
pub(super) fn stage_secret<D: Dut>(
    dut: &mut D,
    actor: &Actor,
    public_key: [u8; 32],
    secret: &[u8],
) -> Result<u64, KmsErrorCode> {
    let begin = unlock_begin(dut, actor, public_key, secret.len() as u16)?;
    for (index, data) in secret.chunks(KMS_SSH_CHUNK_LEN).enumerate() {
        unlock_chunk(dut, actor, begin.handle, index as u32, data)?;
    }
    Ok(begin.handle)
}

pub(super) fn username_field(user: &[u8]) -> [u8; 31] {
    let mut field = [0u8; 31];
    field[..user.len()].copy_from_slice(user);
    field
}

pub(super) fn sign_request(
    actor: &Actor,
    public_key: [u8; 32],
    session_id: [u8; 32],
    user: &[u8],
) -> SshSignRequestPayload {
    SshSignRequestPayload {
        request_id: actor.rid(),
        public_key,
        session_id,
        username_len: user.len() as u8,
        username: username_field(user),
    }
}

pub(super) fn sign<D: Dut>(
    dut: &mut D,
    actor: &Actor,
    public_key: [u8; 32],
    session_id: [u8; 32],
    user: &[u8],
) -> Result<SshSignResponsePayload, KmsErrorCode> {
    let request = sign_request(actor, public_key, session_id, user);
    dut.raw(KmsOpcode::SignSshUserauth, &request.encode(), actor)
        .map(|bytes| SshSignResponsePayload::decode(&bytes).unwrap())
}

pub(super) const SESSION_ID: [u8; 32] = {
    let mut id = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        id[i] = i as u8;
        i += 1;
    }
    id
};

/// Everything a refused operation must leave alone.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Snapshot(String);

pub(super) fn snapshot<D: Dut>(dut: &mut D, principals: &[&Actor]) -> Snapshot {
    let leaf = dut.leaf();
    let identity = leaf
        .identity
        .as_ref()
        .map(|identity| (identity.name().to_vec(), *identity.public_key()));
    let floors: Vec<u64> = principals
        .iter()
        .map(|actor| leaf.floors.floor(actor.principal()))
        .collect();
    Snapshot(format!(
        "{:?}|{:?}|{:?}|{:?}|rev={}|fail={}|ctr={}|{:?}|{}",
        leaf.state,
        leaf.import,
        leaf.unlock,
        identity,
        leaf.revision,
        leaf.failures,
        leaf.handle_counter,
        floors,
        leaf.arena.is_zeroed(),
    ))
}

pub(super) fn service() -> KmsService {
    KmsService::new()
}
