use types::kms::{KmsErrorCode, RELAY_CSR_CHUNK_LEN, RELAY_CSR_MAX_LEN};

use super::SupervisorContext;

/// Approved pending flow: `Prepared -> CsrIssued -> Staged`.
///
/// `Staged` requires the complete ordered CSR consumption plus an
/// authenticated service-net staging call that matches the pending
/// generation; commit is valid only from `Staged`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PendingStage {
    Prepared,
    CsrIssued,
    Staged,
}

/// Pending enrollment created by `BeginRelayEnrollment` and destroyed by
/// commit, abort, any handle violation, or process restart.
#[derive(Debug)]
pub(crate) struct PendingEnrollment {
    pub generation: u64,
    pub policy_epoch: u64,
    pub supervisor: SupervisorContext,
    pub csr_handle: u64,
    pub stage: PendingStage,
    staged_digest: [u8; 32],
    csr: [u8; RELAY_CSR_MAX_LEN],
    csr_len: usize,
    chunks_remaining: u32,
}

impl PendingEnrollment {
    pub(crate) fn new(
        generation: u64,
        policy_epoch: u64,
        supervisor: SupervisorContext,
        csr_handle: u64,
    ) -> Self {
        Self {
            generation,
            policy_epoch,
            supervisor,
            csr_handle,
            stage: PendingStage::Prepared,
            staged_digest: [0; 32],
            csr: [0; RELAY_CSR_MAX_LEN],
            csr_len: 0,
            chunks_remaining: 0,
        }
    }

    /// Move `Prepared -> CsrIssued` once the verified canonical CSR exists.
    pub(crate) fn install(&mut self, csr: &[u8; RELAY_CSR_MAX_LEN], csr_len: usize) {
        debug_assert_eq!(self.stage, PendingStage::Prepared);
        self.csr[..csr_len].copy_from_slice(&csr[..csr_len]);
        self.csr_len = csr_len;
        self.chunks_remaining = self.total_chunks();
        self.stage = PendingStage::CsrIssued;
    }

    fn total_chunks(&self) -> u32 {
        self.csr_len.div_ceil(RELAY_CSR_CHUNK_LEN) as u32
    }

    /// Return the exactly-ordered chunk; only valid from `CsrIssued`.
    pub(crate) fn chunk(
        &mut self,
        chunk_index: u32,
    ) -> Result<([u8; RELAY_CSR_CHUNK_LEN], usize), KmsErrorCode> {
        if self.stage != PendingStage::CsrIssued {
            return Err(KmsErrorCode::CsrHandleInvalid);
        }
        if self.chunks_remaining == 0 {
            // The one-shot handle is exhausted; it no longer names anything.
            return Err(KmsErrorCode::CsrHandleInvalid);
        }
        if chunk_index != self.next_chunk_index() || chunk_index >= self.total_chunks() {
            return Err(KmsErrorCode::CsrOrderInvalid);
        }
        let start = chunk_index as usize * RELAY_CSR_CHUNK_LEN;
        let end = (start + RELAY_CSR_CHUNK_LEN).min(self.csr_len);
        let mut chunk = [0u8; RELAY_CSR_CHUNK_LEN];
        chunk[..end - start].copy_from_slice(&self.csr[start..end]);
        self.chunks_remaining -= 1;
        Ok((chunk, end - start))
    }

    fn next_chunk_index(&self) -> u32 {
        self.total_chunks() - self.chunks_remaining
    }

    /// Move `CsrIssued -> Staged`: every chunk is gone and the live
    /// service-net binding attests a validated profile for this generation.
    pub(crate) fn mark_staged(&mut self, profile_digest: [u8; 32]) -> Result<(), KmsErrorCode> {
        if self.stage != PendingStage::CsrIssued {
            return Err(KmsErrorCode::InvalidRequest);
        }
        if self.chunks_remaining != 0 || profile_digest.iter().all(|byte| *byte == 0) {
            return Err(KmsErrorCode::InvalidRequest);
        }
        self.staged_digest = profile_digest;
        self.stage = PendingStage::Staged;
        Ok(())
    }

    pub(crate) fn staged_digest_matches(&self, profile_digest: [u8; 32]) -> bool {
        self.stage == PendingStage::Staged && self.staged_digest == profile_digest
    }
}

/// Opaque one-shot handle mixed from every binding fact. Not a secret; the
/// binding checks in the lifecycle reader are what enforce access.
pub(crate) fn derive_csr_handle(
    generation: u64,
    policy_epoch: u64,
    request_id: u64,
    begin_counter: u64,
    restart_epoch: u64,
    supervisor: &SupervisorContext,
) -> u64 {
    use blake2::{Blake2s256, Digest};
    let mut hash = Blake2s256::new();
    hash.update(b"cellos-relay-csr-handle-v1");
    hash.update(generation.to_le_bytes());
    hash.update(policy_epoch.to_le_bytes());
    hash.update(request_id.to_le_bytes());
    hash.update(begin_counter.to_le_bytes());
    hash.update(restart_epoch.to_le_bytes());
    hash.update(supervisor.cell_id.to_le_bytes());
    hash.update(supervisor.generation.to_le_bytes());
    hash.update((supervisor.tid as u64).to_le_bytes());
    let digest = hash.finalize();
    let raw = u64::from_le_bytes(digest.as_slice()[..8].try_into().expect("fixed digest"));
    // Never zero; uniqueness comes from the bound facts, not entropy.
    raw | 0x8000_0000_0000_0001
}
