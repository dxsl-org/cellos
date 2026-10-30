//! Per-login unlock slot (opcodes 22-24): one staged passphrase, consumed by
//! exactly one signature attempt.

use types::kms::{
    KmsErrorCode, SshAbortRequestPayload, SshChunkAckPayload, SshChunkRequestPayload,
    SshTransferBeginResponsePayload, SshUnlockBeginRequestPayload, KMS_SSH_CHUNK_LEN,
    KMS_SSH_PUBLIC_KEY_LEN, KMS_SSH_SECRET_MAX,
};

use super::limits::FAIL_LIMIT;
use super::store::Secret;
use super::{Principal, SshLeaf, SshPersist, Transfer, Violation, HANDLE_UNLOCK};

/// Facts bound to the staged secret. The secret bytes themselves are in the
/// leaf's fixed `secret` buffer.
#[derive(Debug, Clone, Copy)]
pub(crate) struct UnlockState {
    pub transfer: Transfer,
    pub public_key: [u8; KMS_SSH_PUBLIC_KEY_LEN],
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum UnlockSlot {
    Empty,
    Collecting(UnlockState),
    Staged(UnlockState),
}

impl<P: SshPersist> SshLeaf<P> {
    /// Op 22. Admission, leaf state, decode and the replay floor are already
    /// checked.
    pub(crate) fn unlock_begin(
        &mut self,
        principal: Principal,
        tid: usize,
        request: &SshUnlockBeginRequestPayload,
    ) -> Result<SshTransferBeginResponsePayload, KmsErrorCode> {
        let identity = self
            .identity
            .as_ref()
            .filter(|identity| *identity.public_key() == request.public_key)
            .ok_or(KmsErrorCode::SshKeyNotFound)?;
        if !matches!(identity.secret(), Secret::Wrapped { .. }) {
            return Err(KmsErrorCode::InvalidRequest);
        }
        // Refused before any work.
        if self.failures >= FAIL_LIMIT {
            return Err(KmsErrorCode::SshUnlockLocked);
        }
        let handle = self.next_handle(HANDLE_UNLOCK, principal, tid)?;
        // Whichever principal owns an existing slot, it is zeroized and
        // replaced so a dead client cannot wedge unlock.
        self.wipe_unlock();
        let transfer = Transfer::new(handle, principal, tid, u32::from(request.secret_len));
        self.unlock = UnlockSlot::Collecting(UnlockState {
            transfer,
            public_key: request.public_key,
        });
        Ok(SshTransferBeginResponsePayload {
            handle,
            total_len: u32::from(request.secret_len),
            chunk_count: transfer.chunk_count,
            chunk_capacity: KMS_SSH_CHUNK_LEN as u16,
            reserved: [0; 6],
        })
    }

    /// Op 23. Any violation destroys the slot and wipes the secret.
    pub(crate) fn unlock_chunk(
        &mut self,
        principal: Principal,
        tid: usize,
        chunk: &SshChunkRequestPayload,
    ) -> Result<SshChunkAckPayload, KmsErrorCode> {
        let verdict = match &self.unlock {
            UnlockSlot::Empty => return Err(KmsErrorCode::SshHandleInvalid),
            UnlockSlot::Staged(_) => Err(Violation::Handle),
            UnlockSlot::Collecting(state) => state.transfer.validate_chunk(chunk, principal, tid),
        };
        let offset = match verdict {
            Ok(offset) => offset,
            Err(violation) => {
                self.wipe_unlock();
                return Err(violation.code());
            }
        };
        let data = chunk.data();
        let end = offset.saturating_add(data.len());
        let Some(dst) = self.secret.get_mut(offset..end) else {
            self.wipe_unlock();
            return Err(KmsErrorCode::SshOrderInvalid);
        };
        dst.copy_from_slice(data);
        let UnlockSlot::Collecting(state) = &mut self.unlock else {
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
            self.unlock = UnlockSlot::Staged(done);
        }
        Ok(ack)
    }

    /// Op 24: destroy the caller's own slot.
    pub(crate) fn unlock_abort(
        &mut self,
        principal: Principal,
        request: &SshAbortRequestPayload,
    ) -> Result<(), KmsErrorCode> {
        match &self.unlock {
            UnlockSlot::Collecting(state) | UnlockSlot::Staged(state)
                if state.transfer.handle == request.handle
                    && state.transfer.principal == principal =>
            {
                self.wipe_unlock();
                Ok(())
            }
            _ => Err(KmsErrorCode::SshHandleInvalid),
        }
    }

    /// Sign step 3 for a wrapped key: copy the staged passphrase for exactly
    /// this principal and key into the caller's wiped-on-drop buffer and
    /// return its length. The slot is consumed (wiped) whatever happens next.
    /// The copy is made in place so no by-value move leaves a second image of
    /// the passphrase on the stack.
    pub(crate) fn take_staged_secret(
        &mut self,
        principal: Principal,
        public_key: &[u8; KMS_SSH_PUBLIC_KEY_LEN],
        out: &mut [u8; KMS_SSH_SECRET_MAX],
    ) -> Result<usize, KmsErrorCode> {
        let staged = match &self.unlock {
            UnlockSlot::Empty => return Err(KmsErrorCode::SshUnlockRequired),
            UnlockSlot::Staged(state)
                if state.transfer.principal == principal && state.public_key == *public_key =>
            {
                *state
            }
            // Collecting, or a slot of another principal or key: destroyed.
            UnlockSlot::Collecting(_) | UnlockSlot::Staged(_) => {
                self.wipe_unlock();
                return Err(KmsErrorCode::SshUnlockRequired);
            }
        };
        if self.failures >= FAIL_LIMIT {
            self.wipe_unlock();
            return Err(KmsErrorCode::SshUnlockLocked);
        }
        let len = staged.transfer.total_len as usize;
        let copied = match (self.secret.get(..len), out.get_mut(..len)) {
            (Some(src), Some(dst)) => {
                dst.copy_from_slice(src);
                true
            }
            _ => false,
        };
        self.wipe_unlock();
        if copied {
            Ok(len)
        } else {
            Err(KmsErrorCode::SshUnlockRequired)
        }
    }
}
