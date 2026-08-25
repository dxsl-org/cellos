use api::caller_identity::CallerIdentity;
use types::kms::{
    KmsCapabilityReadiness, KmsErrorCode, KmsProviderKind, KmsRequestV1,
    RelayProviderAssessment, Tls13ClientCertificateVerifyRequestPayload,
    Tls13ClientCertificateVerifyResponsePayload,
};

use crate::auth::ServiceRegistrySnapshot;
use crate::reply::SuccessPayload;
use crate::storage::{normalize_and_verify_tls13_signature, RelaySignError};

use super::{require_empty, KmsService};

impl KmsService {
    pub(super) fn relay_status(
        &self,
        request: &KmsRequestV1,
        sender: usize,
        caller: Option<CallerIdentity>,
        registry: ServiceRegistrySnapshot,
    ) -> Result<SuccessPayload, KmsErrorCode> {
        require_empty(request)?;
        self.authorize_service_net(sender, caller, registry)?;
        let status = self.provider.relay_p256_status();
        if status.metadata.provider == KmsProviderKind::None {
            return Err(KmsErrorCode::RelayUnavailable);
        }
        SuccessPayload::new(&status.metadata.encode())
    }

    pub(super) fn sign_tls13(
        &mut self,
        request: &KmsRequestV1,
        sender: usize,
        caller: Option<CallerIdentity>,
        registry: ServiceRegistrySnapshot,
    ) -> Result<SuccessPayload, KmsErrorCode> {
        self.authorize_service_net(sender, caller, registry)?;
        let payload = Tls13ClientCertificateVerifyRequestPayload::decode(
            request.payload().map_err(|_| KmsErrorCode::InvalidRequest)?,
        )
        .filter(|payload| payload.request_id != 0)
        .ok_or(KmsErrorCode::InvalidRequest)?;
        if payload.request_id <= self.last_tls_request_id {
            return Err(KmsErrorCode::InvalidRequest);
        }
        let status = self.provider.relay_p256_status();
        if status.metadata.readiness != KmsCapabilityReadiness::Ready {
            return Err(KmsErrorCode::RelayUnavailable);
        }
        if status.metadata.assessment != RelayProviderAssessment::ProductionQualified {
            return Err(KmsErrorCode::QualificationRequired);
        }
        if payload.relay_generation != status.metadata.relay_generation {
            return Err(KmsErrorCode::RelayGenerationMismatch);
        }
        if payload.active_profile_digest != status.metadata.active_profile_digest {
            return Err(KmsErrorCode::ActiveProfileMismatch);
        }
        let signature = self
            .provider
            .sign_tls13_client_certificate_verify(
                payload.transcript_hash,
                payload.relay_generation,
                payload.active_profile_digest,
                payload.request_id,
            )
            .map_err(map_relay_error)?;
        let signature = normalize_and_verify_tls13_signature(
            payload.transcript_hash,
            &status.verifying_key_sec1,
            signature,
        )
        .map_err(|_| KmsErrorCode::InvalidSignature)?;
        let response = SuccessPayload::new(
            &Tls13ClientCertificateVerifyResponsePayload { signature }.encode(),
        )?;
        self.last_tls_request_id = payload.request_id;
        Ok(response)
    }

    fn authorize_service_net(
        &self,
        sender: usize,
        caller: Option<CallerIdentity>,
        registry: ServiceRegistrySnapshot,
    ) -> Result<(), KmsErrorCode> {
        self.service_net_binding
            .ok_or(KmsErrorCode::ServiceBindingRequired)?
            .authorizes(sender, caller, registry.net_tid)
    }
}

fn map_relay_error(error: RelaySignError) -> KmsErrorCode {
    match error {
        RelaySignError::Unavailable => KmsErrorCode::RelayUnavailable,
        RelaySignError::GenerationMismatch => KmsErrorCode::RelayGenerationMismatch,
        RelaySignError::ProfileMismatch => KmsErrorCode::ActiveProfileMismatch,
        RelaySignError::QualificationRequired => KmsErrorCode::QualificationRequired,
        RelaySignError::InvalidRequest => KmsErrorCode::InvalidRequest,
        RelaySignError::Failure => KmsErrorCode::InvalidSignature,
    }
}
