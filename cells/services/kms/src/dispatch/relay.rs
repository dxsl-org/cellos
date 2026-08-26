use api::caller_identity::CallerIdentity;
use types::kms::{
    KmsCapabilityReadiness, KmsErrorCode, KmsProviderKind, KmsRequestV1, RelayProviderAssessment,
    Tls13ClientCertificateVerifyRequestPayload, Tls13ClientCertificateVerifyResponsePayload,
};

use crate::auth::ServiceRegistrySnapshot;
use crate::reply::SuccessPayload;
use crate::storage::{normalize_and_verify_tls13_signature, RelaySignError};

use super::{require_empty, KmsService};

impl KmsService {
    pub(super) fn relay_status(
        &mut self,
        request: &KmsRequestV1,
        sender: usize,
        caller: Option<CallerIdentity>,
        registry: ServiceRegistrySnapshot,
    ) -> Result<SuccessPayload, KmsErrorCode> {
        require_empty(request)?;
        self.authorize_service_net(sender, caller, registry)?;
        let mut metadata = self.provider.relay_p256_status().metadata;
        if metadata.provider == KmsProviderKind::None {
            return Err(KmsErrorCode::RelayUnavailable);
        }
        // The protected lifecycle is authoritative for serving identity.
        match self.lifecycle.serving() {
            Some(active) => {
                metadata.relay_generation = active.generation;
                metadata.policy_epoch = active.policy_epoch;
                metadata.active_profile_digest = active.profile_digest;
            }
            None => {
                metadata.readiness = KmsCapabilityReadiness::Unavailable;
                metadata.relay_generation = 0;
                metadata.active_profile_digest = [0; 32];
            }
        }
        SuccessPayload::new(&metadata.encode())
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
            request
                .payload()
                .map_err(|_| KmsErrorCode::InvalidRequest)?,
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
        if !assessment_authorizes_signing(status.metadata.provider, status.metadata.assessment) {
            return Err(KmsErrorCode::QualificationRequired);
        }
        // The protected lifecycle must be actively serving this exact tuple;
        // revoked or missing state never signs. A generation older than the
        // serving one is retired (key destroyed): unavailable. A newer,
        // never-committed generation is a mismatch.
        let Some(active) = self.lifecycle.serving() else {
            return Err(KmsErrorCode::RelayUnavailable);
        };
        if active.generation == payload.relay_generation {
            if active.profile_digest != payload.active_profile_digest {
                return Err(KmsErrorCode::ActiveProfileMismatch);
            }
        } else if payload.relay_generation < active.generation {
            return Err(KmsErrorCode::RelayUnavailable);
        } else {
            return Err(KmsErrorCode::RelayGenerationMismatch);
        }
        if payload.relay_generation != status.metadata.relay_generation {
            return Err(KmsErrorCode::RelayGenerationMismatch);
        }
        if payload.active_profile_digest != status.metadata.active_profile_digest {
            return Err(KmsErrorCode::ActiveProfileMismatch);
        }
        self.lifecycle
            .observe_authenticated_time(status.metadata.authenticated_time_floor)?;
        self.persist_protected_lifecycle()?;
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

    pub(super) fn authorize_service_net(
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

fn assessment_authorizes_signing(
    provider: KmsProviderKind,
    assessment: RelayProviderAssessment,
) -> bool {
    match (provider, assessment) {
        (_, RelayProviderAssessment::ProductionQualified) => true,
        #[cfg(feature = "development-silo-provider")]
        (KmsProviderKind::SiloWrapped, RelayProviderAssessment::DevelopmentReference) => true,
        _ => false,
    }
}

pub(super) fn map_relay_error(error: RelaySignError) -> KmsErrorCode {
    match error {
        RelaySignError::Unavailable => KmsErrorCode::RelayUnavailable,
        RelaySignError::GenerationMismatch => KmsErrorCode::RelayGenerationMismatch,
        RelaySignError::ProfileMismatch => KmsErrorCode::ActiveProfileMismatch,
        RelaySignError::QualificationRequired => KmsErrorCode::QualificationRequired,
        RelaySignError::InvalidRequest => KmsErrorCode::InvalidRequest,
        RelaySignError::Failure => KmsErrorCode::InvalidSignature,
        RelaySignError::CleanupFailed => KmsErrorCode::RelayUnavailable,
    }
}
