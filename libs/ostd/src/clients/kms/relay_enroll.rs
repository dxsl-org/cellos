use types::kms::{
    validate_hostname, KmsOpcode, RelayActivePublicKeyPayload, RelayCsrChunkRequestPayload,
    RelayCsrChunkResponsePayload, RelayEnrollmentAbortRequestPayload,
    RelayEnrollmentBeginRequestPayload, RelayEnrollmentBeginResponsePayload,
    RelayGenerationCommitRequestPayload, RelayGenerationCommitResponsePayload,
    RelayStageProfileRequestPayload, RELAY_CSR_CHUNK_LEN, RELAY_CSR_MAX_LEN, RELAY_HOSTNAME_MAX,
};

use super::{KmsClient, KmsClientError};

impl KmsClient {
    /// Opcode 9: open enrollment for `hostname` and fetch the CSR handle.
    ///
    /// Supervisor-only on the KMS side; the hostname is validated against
    /// the frozen DNS profile before any frame is built.
    pub fn begin_relay_enrollment(
        &self,
        hostname: &[u8],
    ) -> Result<RelayEnrollmentBeginResponsePayload, KmsClientError> {
        if !validate_hostname(hostname) {
            return Err(KmsClientError::InvalidPayload);
        }
        let mut padded = [0u8; RELAY_HOSTNAME_MAX];
        padded[..hostname.len()].copy_from_slice(hostname);
        let payload = RelayEnrollmentBeginRequestPayload {
            hostname_len: hostname.len() as u8,
            hostname: padded,
        };
        let response = self.call_opcode(KmsOpcode::BeginRelayEnrollment, &payload.encode())?;
        self.decode_payload(&response, RelayEnrollmentBeginResponsePayload::decode)
    }

    /// Opcode 10: one-shot ordered CSR chunk read. Replayed or reordered
    /// handles invalidate the whole pending enrollment server-side.
    pub fn read_relay_csr_chunk(
        &self,
        csr_handle: u64,
        chunk_index: u32,
    ) -> Result<RelayCsrChunkResponsePayload, KmsClientError> {
        let payload = RelayCsrChunkRequestPayload {
            csr_handle,
            chunk_index,
            reserved: 0,
        };
        let response = self.call_opcode(KmsOpcode::ReadRelayCsrChunk, &payload.encode())?;
        self.decode_payload(&response, RelayCsrChunkResponsePayload::decode)
    }

    /// Read the full CSR through strictly ordered chunk reads.
    pub fn read_full_relay_csr(
        &self,
        begin: &RelayEnrollmentBeginResponsePayload,
    ) -> Result<(heapless::Vec<u8, RELAY_CSR_MAX_LEN>, u64), KmsClientError> {
        let total = begin.csr_len as usize;
        if total > RELAY_CSR_MAX_LEN {
            return Err(KmsClientError::InvalidPayload);
        }
        let mut csr = heapless::Vec::<u8, RELAY_CSR_MAX_LEN>::new();
        for index in 0..total.div_ceil(RELAY_CSR_CHUNK_LEN) {
            let chunk = self.read_relay_csr_chunk(begin.csr_handle, index as u32)?;
            let len = chunk.chunk_len as usize;
            if len == 0 || len > RELAY_CSR_CHUNK_LEN || csr.len() + len > total {
                return Err(KmsClientError::InvalidPayload);
            }
            csr.extend_from_slice(&chunk.chunk[..len])
                .map_err(|_| KmsClientError::InvalidPayload)?;
        }
        Ok((csr, begin.csr_handle))
    }

    /// Opcode 13 (service-net only): bind the validated profile digest to
    /// the pending generation before any commit can succeed.
    pub fn stage_relay_profile(
        &self,
        pending_relay_generation: u64,
        expected_policy_epoch: u64,
        profile_digest: &[u8; 32],
    ) -> Result<(), KmsClientError> {
        let payload = RelayStageProfileRequestPayload {
            pending_relay_generation,
            expected_policy_epoch,
            profile_digest: *profile_digest,
        };
        let response = self.call_opcode(KmsOpcode::StageRelayProfile, &payload.encode())?;
        self.expect_empty(&response)
    }

    /// Opcode 11: atomic commit. Valid only from `Staged` with the exact
    /// staged digest.
    pub fn commit_relay_generation(
        &self,
        pending_relay_generation: u64,
        expected_policy_epoch: u64,
        profile_digest: &[u8; 32],
    ) -> Result<RelayGenerationCommitResponsePayload, KmsClientError> {
        let payload = RelayGenerationCommitRequestPayload {
            pending_relay_generation,
            expected_policy_epoch,
            profile_digest: *profile_digest,
        };
        let response = self.call_opcode(KmsOpcode::CommitRelayGeneration, &payload.encode())?;
        self.decode_payload(&response, RelayGenerationCommitResponsePayload::decode)
    }

    /// Opcode 12: destroy the named pending generation.
    pub fn abort_relay_enrollment(
        &self,
        pending_relay_generation: u64,
    ) -> Result<(), KmsClientError> {
        let payload = RelayEnrollmentAbortRequestPayload {
            pending_relay_generation,
        };
        let response = self.call_opcode(KmsOpcode::AbortRelayEnrollment, &payload.encode())?;
        self.expect_empty(&response)
    }

    /// Opcode 14 (service-net only): the active generation's SEC1 point and
    /// its SHA-256 — never pending or private material.
    pub fn get_relay_active_public_key(
        &self,
    ) -> Result<RelayActivePublicKeyPayload, KmsClientError> {
        let response = self.call_opcode(KmsOpcode::GetRelayActivePublicKey, &[])?;
        self.decode_payload(&response, RelayActivePublicKeyPayload::decode)
    }
}
