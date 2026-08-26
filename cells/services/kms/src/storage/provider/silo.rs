use ostd::syscall::{sys_lookup_service, sys_recv_timeout, sys_send, SyscallResult};
use types::kms::{
    KmsCapabilityReadiness, KmsKeyAlgorithm, KmsProviderKind, RelayP256StatusPayload,
    RelayProviderAssessment,
};
use types::silo::{
    DevelopmentSiloError, DevelopmentSiloRequest, DevelopmentSiloResponse,
    DEVELOPMENT_PROFILE_DIGEST, DEVELOPMENT_RELAY_GENERATION,
    DEVELOPMENT_SILO_FRAME_LEN,
};

use super::super::capability::{RelayP256Status, RelaySignError};

/// Stateful internal KMS client for the development-only Silo provider.
#[derive(Debug)]
pub(crate) struct DevelopmentSiloProvider {
    next_request_seq: u64,
    last_response_seq: u64,
    failed_closed: bool,
}

impl DevelopmentSiloProvider {
    pub(crate) const fn new() -> Self {
        Self {
            next_request_seq: 1,
            last_response_seq: 0,
            failed_closed: false,
        }
    }

    pub(crate) fn status(&mut self) -> RelayP256Status {
        let request_seq = match self.take_request_seq() {
            Ok(seq) => seq,
            Err(_) => return RelayP256Status::unavailable(),
        };
        let request = DevelopmentSiloRequest::RelayStatus { request_seq };
        let verifying_key_sec1 = match self.call(request) {
            Ok(DevelopmentSiloResponse::RelayStatus { verifying_key_sec1, .. }) => verifying_key_sec1,
            _ => return RelayP256Status::unavailable(),
        };
        RelayP256Status {
            metadata: RelayP256StatusPayload {
                algorithm: KmsKeyAlgorithm::RelayP256Sha256,
                readiness: KmsCapabilityReadiness::Ready,
                provider: KmsProviderKind::SiloWrapped,
                assessment: RelayProviderAssessment::DevelopmentReference,
                reserved: 0,
                relay_generation: DEVELOPMENT_RELAY_GENERATION,
                policy_epoch: 1,
                authenticated_time_floor: 0,
                qualification_epoch: 0,
                active_profile_digest: DEVELOPMENT_PROFILE_DIGEST,
                qualification_record_digest: [0; 32],
            },
            verifying_key_sec1,
        }
    }

    pub(crate) fn sign(
        &mut self,
        transcript_hash: [u8; 32],
        relay_generation: u64,
        active_profile_digest: [u8; 32],
        request_id: u64,
    ) -> Result<[u8; 64], RelaySignError> {
        let request_seq = self.take_request_seq()?;
        let request = DevelopmentSiloRequest::SignTls13ClientCertificateVerify {
            request_seq,
            transcript_hash,
            relay_generation,
            active_profile_digest,
            request_id,
        };
        match self.call(request)? {
            DevelopmentSiloResponse::Tls13ClientCertificateVerify { signature, .. } => Ok(signature),
            DevelopmentSiloResponse::Error { error, .. } => Err(map_error(error)),
            _ => self.fail(RelaySignError::Failure),
        }
    }

    fn take_request_seq(&mut self) -> Result<u64, RelaySignError> {
        if self.failed_closed {
            return Err(RelaySignError::Unavailable);
        }
        let current = self.next_request_seq;
        self.next_request_seq = current.checked_add(1).filter(|seq| *seq != 0)
            .ok_or(RelaySignError::Unavailable)?;
        Ok(current)
    }

    fn call(&mut self, request: DevelopmentSiloRequest) -> Result<DevelopmentSiloResponse, RelaySignError> {
        let silo_tid = sys_lookup_service(api::syscall::service::SILO)
            .ok_or(RelaySignError::Unavailable)?;
        match sys_send(silo_tid, &request.encode()) {
            SyscallResult::Ok(_) => {}
            _ => return self.fail(RelaySignError::Unavailable),
        }
        let mut bytes = [0u8; DEVELOPMENT_SILO_FRAME_LEN];
        match sys_recv_timeout(silo_tid, &mut bytes, 8) {
            SyscallResult::Ok(sender) if sender == silo_tid => {}
            _ => return self.fail(RelaySignError::Unavailable),
        }
        let response = match DevelopmentSiloResponse::decode(&bytes) {
            Some(response) => response,
            None => return self.fail(RelaySignError::Failure),
        };
        let (request_seq, response_seq) = response_sequences(response);
        if request_seq != request.request_seq() || response_seq <= self.last_response_seq {
            return self.fail(RelaySignError::Failure);
        }
        self.last_response_seq = response_seq;
        Ok(response)
    }

    fn fail<T>(&mut self, error: RelaySignError) -> Result<T, RelaySignError> {
        self.failed_closed = true;
        Err(error)
    }
}

fn response_sequences(response: DevelopmentSiloResponse) -> (u64, u64) {
    match response {
        DevelopmentSiloResponse::RelayStatus { request_seq, response_seq, .. }
        | DevelopmentSiloResponse::Tls13ClientCertificateVerify { request_seq, response_seq, .. }
        | DevelopmentSiloResponse::Error { request_seq, response_seq, .. } => (request_seq, response_seq),
    }
}

fn map_error(error: DevelopmentSiloError) -> RelaySignError {
    match error {
        DevelopmentSiloError::GenerationMismatch => RelaySignError::GenerationMismatch,
        DevelopmentSiloError::ProfileMismatch => RelaySignError::ProfileMismatch,
        DevelopmentSiloError::Malformed | DevelopmentSiloError::Sequence => RelaySignError::InvalidRequest,
        DevelopmentSiloError::Unauthorized | DevelopmentSiloError::GuestFault => RelaySignError::Failure,
        DevelopmentSiloError::Unavailable => RelaySignError::Unavailable,
    }
}
