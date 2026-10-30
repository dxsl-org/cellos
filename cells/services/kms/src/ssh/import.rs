//! Import state machine (opcodes 15-18) and the retained staging arena.

extern crate alloc;

use alloc::vec::Vec;

use sha2::{Digest, Sha256};
use types::kms::{
    KmsCapabilityReadiness, KmsErrorCode, SshAbortRequestPayload, SshChunkAckPayload,
    SshChunkRequestPayload, SshImportBeginRequestPayload, SshImportCommitRequestPayload,
    SshKeyStatusPayload, SshTransferBeginResponsePayload, KMS_SSH_CHUNK_LEN, KMS_SSH_KEY_NAME_MAX,
};
use zeroize::Zeroize;

use super::limits::STAGING_LEN;
use super::parser::{parse_outer, Cipher, Reject};
use super::store::{key_status, StoredIdentity};
use super::unwrap::plain_seed;
use super::{LeafState, Principal, SshLeaf, SshPersist, Stored, Transfer, HANDLE_IMPORT};
use types::kms::KmsCustodyPosture;

/// One 64 KiB staging buffer, allocated lazily on the first Begin, retained
/// for the life of the cell and wiped (never freed) on every exit: no heap
/// fragmentation in the 1 MiB cell heap.
pub(crate) struct Arena {
    buf: Vec<u8>,
}

impl Arena {
    pub(crate) const fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// Allocate on first use. Failure leaves the arena empty.
    pub(crate) fn ensure(&mut self) -> Result<(), ()> {
        if self.buf.len() == STAGING_LEN {
            return Ok(());
        }
        self.buf.try_reserve_exact(STAGING_LEN).map_err(|_| ())?;
        self.buf.resize(STAGING_LEN, 0);
        Ok(())
    }

    pub(crate) fn wipe(&mut self) {
        self.buf.as_mut_slice().zeroize();
    }

    #[cfg(test)]
    pub(crate) fn is_zeroed(&self) -> bool {
        self.buf.iter().all(|byte| *byte == 0)
    }

    #[cfg(test)]
    pub(crate) fn is_allocated(&self) -> bool {
        self.buf.len() == STAGING_LEN
    }

    fn write(&mut self, offset: usize, data: &[u8]) -> bool {
        let Some(end) = offset.checked_add(data.len()) else {
            return false;
        };
        match self.buf.get_mut(offset..end) {
            Some(dst) => {
                dst.copy_from_slice(data);
                true
            }
            None => false,
        }
    }

    fn prefix(&self, len: usize) -> Option<&[u8]> {
        self.buf.get(..len)
    }
}

impl Drop for Arena {
    fn drop(&mut self) {
        self.buf.as_mut_slice().zeroize();
    }
}

/// Facts recorded at Begin. No secret: the blob itself lives only in the arena.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ImportState {
    pub transfer: Transfer,
    pub name: [u8; KMS_SSH_KEY_NAME_MAX],
    pub name_len: u8,
    pub blob_sha256: [u8; 32],
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ImportSlot {
    Idle,
    Receiving(ImportState),
    Complete(ImportState),
}

impl ImportSlot {
    fn state(&self) -> Option<&ImportState> {
        match self {
            ImportSlot::Idle => None,
            ImportSlot::Receiving(state) | ImportSlot::Complete(state) => Some(state),
        }
    }
}

impl<P: SshPersist> SshLeaf<P> {
    /// Op 15. The caller has already checked admission, leaf state, decode and
    /// the replay floor.
    pub(crate) fn import_begin(
        &mut self,
        principal: Principal,
        tid: usize,
        request: &SshImportBeginRequestPayload,
    ) -> Result<SshTransferBeginResponsePayload, KmsErrorCode> {
        if let Some(identity) = &self.identity {
            return Err(if identity.name() == request.name() {
                KmsErrorCode::SshKeyExists
            } else {
                KmsErrorCode::SshStoreFull
            });
        }
        if self
            .import
            .state()
            .is_some_and(|state| state.transfer.principal == principal)
        {
            return Err(KmsErrorCode::SshImportPending);
        }
        self.arena.ensure().map_err(|_| KmsErrorCode::Busy)?;
        let handle = self.next_handle(HANDLE_IMPORT, principal, tid)?;
        // Every other check passed: another principal's open import (a dead
        // admin instance) is zeroized and replaced.
        self.wipe_import();
        let transfer = Transfer::new(handle, principal, tid, request.total_len);
        self.import = ImportSlot::Receiving(ImportState {
            transfer,
            name: request.name,
            name_len: request.name_len,
            blob_sha256: request.blob_sha256,
        });
        Ok(SshTransferBeginResponsePayload {
            handle,
            total_len: request.total_len,
            chunk_count: transfer.chunk_count,
            chunk_capacity: KMS_SSH_CHUNK_LEN as u16,
            reserved: [0; 6],
        })
    }

    /// Op 16. Any handle, principal, task, order or length violation destroys
    /// the pending import.
    pub(crate) fn import_chunk(
        &mut self,
        principal: Principal,
        tid: usize,
        chunk: &SshChunkRequestPayload,
    ) -> Result<SshChunkAckPayload, KmsErrorCode> {
        let verdict = match &self.import {
            ImportSlot::Idle => return Err(KmsErrorCode::SshHandleInvalid),
            ImportSlot::Complete(_) => Err(super::Violation::Handle),
            ImportSlot::Receiving(state) => state.transfer.validate_chunk(chunk, principal, tid),
        };
        let offset = match verdict {
            Ok(offset) => offset,
            Err(violation) => {
                self.wipe_import();
                return Err(violation.code());
            }
        };
        if !self.arena.write(offset, chunk.data()) {
            self.wipe_import();
            return Err(KmsErrorCode::SshOrderInvalid);
        }
        let ImportSlot::Receiving(state) = &mut self.import else {
            return Err(KmsErrorCode::SshHandleInvalid);
        };
        state.transfer.advance(chunk.chunk_len);
        let ack = SshChunkAckPayload {
            handle: state.transfer.handle,
            next_chunk_index: state.transfer.next_index,
            received_len: state.transfer.received,
        };
        if state.transfer.is_complete() {
            let done = *state;
            self.import = ImportSlot::Complete(done);
        }
        Ok(ack)
    }

    /// Op 17. Consumes the import whatever the outcome: the arena is wiped and
    /// the slot is `Idle` on every path.
    pub(crate) fn import_commit(
        &mut self,
        principal: Principal,
        tid: usize,
        request: &SshImportCommitRequestPayload,
    ) -> Result<SshKeyStatusPayload, KmsErrorCode> {
        let Some((state, complete)) = (match &self.import {
            ImportSlot::Idle => None,
            ImportSlot::Receiving(state) => Some((*state, false)),
            ImportSlot::Complete(state) => Some((*state, true)),
        }) else {
            return Err(KmsErrorCode::SshHandleInvalid);
        };
        let result = self.commit_inner(state, complete, principal, tid, request);
        self.wipe_import();
        result
    }

    fn commit_inner(
        &mut self,
        state: ImportState,
        complete: bool,
        principal: Principal,
        tid: usize,
        request: &SshImportCommitRequestPayload,
    ) -> Result<SshKeyStatusPayload, KmsErrorCode> {
        if !state.transfer.owned_by(request.handle, principal, tid) {
            return Err(KmsErrorCode::SshHandleInvalid);
        }
        if !complete
            || request.chunk_count != state.transfer.chunk_count
            || !state.transfer.is_complete()
        {
            return Err(KmsErrorCode::SshOrderInvalid);
        }
        // Integrity first: no parsing of a corrupt transfer.
        let total = state.transfer.total_len as usize;
        let blob = self
            .arena
            .prefix(total)
            .ok_or(KmsErrorCode::SshOrderInvalid)?;
        let digest: [u8; 32] = Sha256::digest(blob).into();
        if digest != state.blob_sha256 || digest != request.blob_sha256 {
            return Err(KmsErrorCode::SshImportIntegrity);
        }
        let name = state
            .name
            .get(..usize::from(state.name_len))
            .ok_or(KmsErrorCode::SshOrderInvalid)?;
        let identity = match parse_outer(blob) {
            Ok(outer) => match outer.cipher {
                Cipher::None => {
                    let seed = plain_seed(outer.private, Cipher::None, &outer.public)
                        .map_err(|_| KmsErrorCode::SshKeyFormatRejected)?;
                    StoredIdentity::plain(name, outer.public, &seed)
                }
                Cipher::Aes256Ctr => StoredIdentity::wrapped(
                    name,
                    outer.public,
                    outer.salt,
                    outer.rounds,
                    outer.private,
                ),
            },
            Err(Reject::WrappedTooLarge) => return Err(KmsErrorCode::SshStoreFull),
            Err(_) => return Err(KmsErrorCode::SshKeyFormatRejected),
        }
        .ok_or(KmsErrorCode::SshStoreFull)?;
        // Defence: with one identity per store this is unreachable through
        // the ABI (Begin already refused).
        if let Some(existing) = &self.identity {
            return Err(if existing.public_key() == identity.public_key() {
                KmsErrorCode::SshKeyExists
            } else {
                KmsErrorCode::SshStoreFull
            });
        }
        let revision = self.next_revision()?;
        // Persist before the RAM swap: a failure stores nothing.
        let stored = self
            .persist
            .store(&identity.record_view(revision))
            .map_err(|_| KmsErrorCode::PersistFailed)?;
        self.posture = match stored {
            Stored::Written => KmsCustodyPosture::DevPosturePersistent,
            Stored::VolatileOnly => KmsCustodyPosture::DevPostureVolatile,
        };
        let status = key_status(
            &identity,
            KmsCapabilityReadiness::Ready,
            self.posture,
            revision,
        );
        self.identity = Some(identity);
        self.revision = revision;
        self.failures = 0;
        self.wipe_unlock();
        self.state = LeafState::Serving;
        Ok(status)
    }

    /// Op 18: destroy the caller's own pending import.
    pub(crate) fn import_abort(
        &mut self,
        principal: Principal,
        request: &SshAbortRequestPayload,
    ) -> Result<(), KmsErrorCode> {
        match self.import.state() {
            Some(state)
                if state.transfer.handle == request.handle
                    && state.transfer.principal == principal =>
            {
                self.wipe_import();
                Ok(())
            }
            _ => Err(KmsErrorCode::SshHandleInvalid),
        }
    }

    /// `revision + 1`; at the limit the leaf becomes unavailable.
    pub(crate) fn next_revision(&mut self) -> Result<u64, KmsErrorCode> {
        match self.revision.checked_add(1) {
            Some(next) => Ok(next),
            None => {
                self.state = LeafState::Unavailable;
                Err(KmsErrorCode::SshKeyUnavailable)
            }
        }
    }
}
