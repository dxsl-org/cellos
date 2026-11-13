//! Staged trust queries (opcodes 25-27, 29), framed like an SSH import.
//!
//! `BeginTrustQuery` records the kind, port, field lengths, session and epoch and the
//! SHA-256 of `host || algorithm || key`, and answers a handle with the canonical
//! 96-byte chunk plan. Chunks arrive strictly in order with their canonical lengths; a
//! violation by the owner destroys the query. `CommitTrustQuery` consumes the query
//! whatever the outcome and hands the verified fields on only when every chunk arrived
//! and both the declared and the echoed digest match.
//!
//! One open query per caller principal (a new Begin replaces it, and any query of an
//! earlier generation of the same cell) and at most [`MAX_QUERIES`] in total; a query
//! older than [`QUERY_LIFETIME_NS`] gives way to a new one.

extern crate alloc;

use alloc::vec::Vec;

use api::cell_session::ObjectRefV1 as Ref;
use kms_codec::{
    ssh_chunk_count, ssh_chunk_len, KmsErrorCode, KmsTrustQueryKind, SshChunkAckPayload,
    SshChunkRequestPayload, SshImportCommitRequestPayload, SshTransferBeginResponsePayload,
    TrustQueryBeginRequestPayload, KMS_SSH_CHUNK_LEN,
};
use sha2::{Digest, Sha256};

use super::{owner, Principal};

/// Open queries across all callers.
pub(super) const MAX_QUERIES: usize = 4;
/// A query is a few frames long; one this old was abandoned by its caller.
pub(super) const QUERY_LIFETIME_NS: u64 = 10_000_000_000;

struct Query {
    handle: u64,
    owner: Principal,
    begin: TrustQueryBeginRequestPayload,
    total: u32,
    chunk_count: u32,
    next_index: u32,
    bytes: Vec<u8>,
    opened_ns: u64,
}

pub(super) struct Queries {
    open: Vec<Query>,
    next_handle: u64,
}

/// A committed query: its verified fields.
pub(super) struct Staged {
    pub kind: KmsTrustQueryKind,
    pub port: u16,
    pub epoch: u64,
    pub session: Ref,
    host_len: usize,
    algorithm_len: usize,
    bytes: Vec<u8>,
}

impl Staged {
    /// `(host, algorithm, key)`; host and algorithm must be UTF-8.
    pub fn fields(&self) -> Result<(&str, &str, &[u8]), KmsErrorCode> {
        let (host, rest) = self.bytes.split_at(self.host_len);
        let (algorithm, key) = rest.split_at(self.algorithm_len);
        match (core::str::from_utf8(host), core::str::from_utf8(algorithm)) {
            (Ok(host), Ok(algorithm)) => Ok((host, algorithm, key)),
            _ => Err(KmsErrorCode::InvalidRequest),
        }
    }
}

impl Queries {
    pub const fn new() -> Self {
        Self {
            open: Vec::new(),
            next_handle: 1,
        }
    }

    fn owned(&self, principal: Principal, handle: u64) -> Result<usize, KmsErrorCode> {
        self.open
            .iter()
            .position(|q| q.handle == handle && q.owner == principal)
            .ok_or(KmsErrorCode::TrustHandleInvalid)
    }

    /// Op 25.
    pub fn begin(
        &mut self,
        principal: Principal,
        request: &TrustQueryBeginRequestPayload,
    ) -> Result<SshTransferBeginResponsePayload, KmsErrorCode> {
        let now = owner::now_ns().unwrap_or(0);
        self.open.retain(|q| {
            q.owner.cell_id != principal.cell_id
                && now < q.opened_ns.saturating_add(QUERY_LIFETIME_NS)
        });
        if self.open.len() >= MAX_QUERIES {
            return Err(KmsErrorCode::Busy);
        }
        let handle = self.next_handle;
        let next = handle.checked_add(1).ok_or(KmsErrorCode::Busy)?;
        let total = request.total_len();
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(total as usize)
            .map_err(|_| KmsErrorCode::Busy)?;
        self.next_handle = next;
        let chunk_count = ssh_chunk_count(total);
        self.open.push(Query {
            handle,
            owner: principal,
            begin: *request,
            total,
            chunk_count,
            next_index: 0,
            bytes,
            opened_ns: now,
        });
        Ok(SshTransferBeginResponsePayload {
            handle,
            total_len: total,
            chunk_count,
            chunk_capacity: KMS_SSH_CHUNK_LEN as u16,
            reserved: [0; 6],
        })
    }

    /// Op 26. Another caller's or an unknown handle changes nothing; the owner's out-of-
    /// order or wrong-length chunk destroys the query.
    pub fn chunk(
        &mut self,
        principal: Principal,
        chunk: &SshChunkRequestPayload,
    ) -> Result<SshChunkAckPayload, KmsErrorCode> {
        let n = self.owned(principal, chunk.handle)?;
        let q = &mut self.open[n];
        if chunk.chunk_index != q.next_index
            || ssh_chunk_len(q.total, chunk.chunk_index) != Some(chunk.chunk_len)
        {
            self.open.swap_remove(n);
            return Err(KmsErrorCode::TrustOrderInvalid);
        }
        q.bytes.extend_from_slice(chunk.data());
        q.next_index += 1;
        Ok(SshChunkAckPayload {
            handle: q.handle,
            next_chunk_index: q.next_index,
            received_len: q.bytes.len() as u32,
        })
    }

    /// Op 27: consumes the query whatever the outcome.
    pub fn commit(
        &mut self,
        principal: Principal,
        request: &SshImportCommitRequestPayload,
    ) -> Result<Staged, KmsErrorCode> {
        let n = self.owned(principal, request.handle)?;
        let q = self.open.swap_remove(n);
        if q.next_index != q.chunk_count
            || request.chunk_count != q.chunk_count
            || q.bytes.len() != q.total as usize
        {
            return Err(KmsErrorCode::TrustOrderInvalid);
        }
        let digest: [u8; 32] = Sha256::digest(&q.bytes).into();
        if digest != q.begin.query_sha256 || digest != request.blob_sha256 {
            return Err(KmsErrorCode::InvalidRequest);
        }
        let (id, generation) = q.begin.session;
        Ok(Staged {
            kind: q.begin.kind,
            port: q.begin.port,
            epoch: q.begin.epoch,
            session: Ref { id, generation },
            host_len: usize::from(q.begin.host_len),
            algorithm_len: usize::from(q.begin.algorithm_len),
            bytes: q.bytes,
        })
    }

    /// Op 29: destroy the caller's own query.
    pub fn abort(&mut self, principal: Principal, handle: u64) -> Result<(), KmsErrorCode> {
        let n = self.owned(principal, handle)?;
        self.open.swap_remove(n);
        Ok(())
    }

    #[cfg(test)]
    pub fn open(&self) -> usize {
        self.open.len()
    }
}
