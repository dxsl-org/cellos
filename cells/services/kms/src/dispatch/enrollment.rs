use api::caller_identity::CallerIdentity;
use sha2::{Digest, Sha256};
use types::kms::{
    p256_spki_der, KmsErrorCode, KmsRequestV1, RelayActivePublicKeyPayload,
    RelayCsrChunkRequestPayload, RelayCsrChunkResponsePayload, RelayEnrollmentAbortRequestPayload,
    RelayEnrollmentBeginRequestPayload, RelayEnrollmentBeginResponsePayload,
    RelayGenerationCommitRequestPayload, RelayGenerationCommitResponsePayload,
    RelayStageProfileRequestPayload,
};

use crate::auth::{authorize_supervisor, ServiceRegistrySnapshot};
use crate::lifecycle::SupervisorContext;
use crate::reply::SuccessPayload;
use crate::storage::verify_and_assemble_csr;

use super::relay::map_relay_error;
use super::{require_empty, KmsService};

fn authorize_enrollment_supervisor(
    sender: usize,
    caller: Option<CallerIdentity>,
    live_supervisor_tid: Option<usize>,
) -> Result<SupervisorContext, KmsErrorCode> {
    authorize_supervisor(sender, caller, live_supervisor_tid)?;
    let caller = caller.expect("authorized");
    Ok(SupervisorContext::from_parts(
        caller.cell_id,
        caller.generation,
        sender,
    ))
}

impl KmsService {
    fn reconcile_pending_cleanup(&mut self) -> Result<(), KmsErrorCode> {
        let Some(generation) = self
            .lifecycle
            .cleanup_pending()
            .map(|pending| pending.generation)
        else {
            return Ok(());
        };
        self.provider
            .destroy_enrollment_key(generation)
            .map_err(|_| KmsErrorCode::RelayUnavailable)?;
        self.lifecycle.confirm_cleanup();
        Ok(())
    }

    /// Opcode 9: supervisor-only enrollment open plus canonical CSR publish.
    pub(super) fn begin_enrollment(
        &mut self,
        request: &KmsRequestV1,
        sender: usize,
        caller: Option<CallerIdentity>,
        registry: ServiceRegistrySnapshot,
    ) -> Result<SuccessPayload, KmsErrorCode> {
        let supervisor = authorize_enrollment_supervisor(sender, caller, registry.supervisor_tid)?;
        let payload = RelayEnrollmentBeginRequestPayload::decode(
            request
                .payload()
                .map_err(|_| KmsErrorCode::InvalidRequest)?,
        )
        .ok_or(KmsErrorCode::InvalidRequest)?;
        // A poisoned slot is a cleanup tombstone. A fresh Begin may replace it
        // only after the provider confirms deletion or prior absence.
        self.reconcile_pending_cleanup()?;
        // Deny before touching the provider whenever the slot is unavailable.
        let (generation, policy_epoch) = self
            .lifecycle
            .open_pending(supervisor, request.request_id)?;
        let proof = match self
            .provider
            .begin_enrollment(generation, payload.hostname())
        {
            Ok(proof) => proof,
            Err(crate::storage::RelaySignError::CleanupFailed) => {
                self.lifecycle.mark_cleanup_required();
                return Err(KmsErrorCode::RelayUnavailable);
            }
            Err(error) => {
                self.lifecycle.drop_pending();
                return Err(map_relay_error(error));
            }
        };
        let assembled =
            match verify_and_assemble_csr(payload.hostname(), &proof.spki_sec1, proof.signature) {
                Ok(assembled) => assembled,
                Err(_) => {
                    self.lifecycle.mark_cleanup_required();
                    if self.reconcile_pending_cleanup().is_err() {
                        return Err(KmsErrorCode::RelayUnavailable);
                    }
                    return Err(KmsErrorCode::InvalidSignature);
                }
            };
        let csr_sha256 = assembled.sha256();
        let csr_handle = self.lifecycle.pending().expect("just opened").csr_handle;
        self.lifecycle.install_csr(&assembled.bytes, assembled.len);
        let response = RelayEnrollmentBeginResponsePayload {
            pending_relay_generation: generation,
            policy_epoch,
            restart_epoch: self.lifecycle.restart_epoch(),
            csr_handle,
            csr_len: assembled.len as u32,
            reserved: 0,
            csr_sha256,
        };
        SuccessPayload::new(&response.encode())
    }

    /// Opcode 10: one-shot ordered CSR chunk read bound to the live supervisor.
    pub(super) fn read_relay_csr_chunk(
        &mut self,
        request: &KmsRequestV1,
        sender: usize,
        caller: Option<CallerIdentity>,
        registry: ServiceRegistrySnapshot,
    ) -> Result<SuccessPayload, KmsErrorCode> {
        let supervisor = authorize_enrollment_supervisor(sender, caller, registry.supervisor_tid)?;
        let payload = RelayCsrChunkRequestPayload::decode(
            request
                .payload()
                .map_err(|_| KmsErrorCode::InvalidRequest)?,
        )
        .ok_or(KmsErrorCode::InvalidRequest)?;
        let result =
            self.lifecycle
                .read_chunk(payload.csr_handle, payload.chunk_index, &supervisor);
        let (chunk, len) = match result {
            Ok(output) => output,
            Err(code) => {
                if self.lifecycle.cleanup_pending().is_some()
                    && self.reconcile_pending_cleanup().is_err()
                {
                    return Err(KmsErrorCode::RelayUnavailable);
                }
                return Err(code);
            }
        };
        let response = RelayCsrChunkResponsePayload {
            chunk_index: payload.chunk_index,
            chunk_len: len as u16,
            reserved: 0,
            chunk,
        };
        SuccessPayload::new(&response.encode())
    }

    /// Opcode 11: atomic commit; promotes the provider key between lifecycle
    /// validation and activation so failure leaves no mixed state.
    pub(super) fn commit_relay_generation(
        &mut self,
        request: &KmsRequestV1,
        sender: usize,
        caller: Option<CallerIdentity>,
        registry: ServiceRegistrySnapshot,
    ) -> Result<SuccessPayload, KmsErrorCode> {
        let supervisor = authorize_enrollment_supervisor(sender, caller, registry.supervisor_tid)?;
        let payload = RelayGenerationCommitRequestPayload::decode(
            request
                .payload()
                .map_err(|_| KmsErrorCode::InvalidRequest)?,
        )
        .ok_or(KmsErrorCode::InvalidRequest)?;
        self.lifecycle.prepare_commit(
            payload.pending_relay_generation,
            payload.expected_policy_epoch,
            payload.profile_digest,
            &supervisor,
        )?;
        self.provider
            .commit_enrollment(payload.pending_relay_generation, payload.profile_digest)
            .map_err(map_relay_error)?;
        let committed = self.lifecycle.apply_commit(
            payload.pending_relay_generation,
            payload.expected_policy_epoch,
            payload.profile_digest,
            &supervisor,
        );
        match committed {
            Ok(committed) => {
                if let Err(code) = self.persist_protected_lifecycle() {
                    self.lifecycle.seal();
                    return Err(code);
                }
                SuccessPayload::new(
                    &RelayGenerationCommitResponsePayload {
                        active_relay_generation: committed.generation,
                        policy_epoch: committed.policy_epoch,
                        active_profile_digest: committed.profile_digest,
                    }
                    .encode(),
                )
            }
            Err(code) => {
                // Promotion without a matching protected transition is never
                // allowed to serve.
                self.lifecycle.seal();
                Err(code)
            }
        }
    }

    /// Opcode 12: abort destroys the pending slot and its provider key.
    pub(super) fn abort_relay_enrollment(
        &mut self,
        request: &KmsRequestV1,
        sender: usize,
        caller: Option<CallerIdentity>,
        registry: ServiceRegistrySnapshot,
    ) -> Result<SuccessPayload, KmsErrorCode> {
        let supervisor = authorize_enrollment_supervisor(sender, caller, registry.supervisor_tid)?;
        let payload = RelayEnrollmentAbortRequestPayload::decode(
            request
                .payload()
                .map_err(|_| KmsErrorCode::InvalidRequest)?,
        )
        .ok_or(KmsErrorCode::InvalidRequest)?;
        {
            let pending = self
                .lifecycle
                .pending()
                .ok_or(KmsErrorCode::CsrHandleInvalid)?;
            if pending.generation != payload.pending_relay_generation {
                return Err(KmsErrorCode::RelayGenerationMismatch);
            }
            if !pending.supervisor.matches(&supervisor) {
                return Err(KmsErrorCode::PermissionDenied);
            }
        }
        self.provider
            .destroy_enrollment_key(payload.pending_relay_generation)
            .map_err(|_| KmsErrorCode::RelayUnavailable)?;
        self.lifecycle.drop_pending();
        SuccessPayload::new(&[])
    }

    /// Opcode 13: service-net binds its validated profile digest.
    pub(super) fn stage_relay_profile(
        &mut self,
        request: &KmsRequestV1,
        sender: usize,
        caller: Option<CallerIdentity>,
        registry: ServiceRegistrySnapshot,
    ) -> Result<SuccessPayload, KmsErrorCode> {
        self.authorize_service_net(sender, caller, registry)?;
        let payload = RelayStageProfileRequestPayload::decode(
            request
                .payload()
                .map_err(|_| KmsErrorCode::InvalidRequest)?,
        )
        .ok_or(KmsErrorCode::InvalidRequest)?;
        self.lifecycle.stage_pending(
            payload.pending_relay_generation,
            payload.expected_policy_epoch,
            payload.profile_digest,
        )?;
        SuccessPayload::new(&[])
    }

    /// Opcode 14: public active-generation key facts for service-net only.
    pub(super) fn get_relay_active_public_key(
        &mut self,
        request: &KmsRequestV1,
        sender: usize,
        caller: Option<CallerIdentity>,
        registry: ServiceRegistrySnapshot,
    ) -> Result<SuccessPayload, KmsErrorCode> {
        require_empty(request)?;
        self.authorize_service_net(sender, caller, registry)?;
        let active = self
            .lifecycle
            .serving()
            .ok_or(KmsErrorCode::RelayUnavailable)?;
        let status = self.provider.relay_p256_status();
        let (spki, spki_len) =
            p256_spki_der(&status.verifying_key_sec1).ok_or(KmsErrorCode::RelayUnavailable)?;
        SuccessPayload::new(
            &RelayActivePublicKeyPayload {
                relay_generation: active.generation,
                spki_sec1: status.verifying_key_sec1,
                spki_sha256: Sha256::digest(&spki[..spki_len]).into(),
            }
            .encode(),
        )
    }
}
