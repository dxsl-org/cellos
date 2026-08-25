use types::kms::{
    KmsCapabilityReadiness, KmsKeyAlgorithm, KmsProviderKind, RelayP256StatusPayload,
    RelayProviderAssessment,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct C2cX25519Status {
    pub(crate) algorithm: KmsKeyAlgorithm,
    pub(crate) generation: u64,
    pub(crate) policy_epoch: u64,
    pub(crate) provider: KmsProviderKind,
    pub(crate) assessment: RelayProviderAssessment,
    pub(crate) readiness: KmsCapabilityReadiness,
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RelayP256Status {
    pub(crate) metadata: RelayP256StatusPayload,
    pub(crate) verifying_key_sec1: [u8; 65],
}

impl RelayP256Status {
    pub(crate) const fn unavailable() -> Self {
        Self {
            metadata: RelayP256StatusPayload {
                algorithm: KmsKeyAlgorithm::RelayP256Sha256,
                readiness: KmsCapabilityReadiness::Unavailable,
                provider: KmsProviderKind::None,
                assessment: RelayProviderAssessment::Unassessed,
                reserved: 0,
                relay_generation: 0,
                policy_epoch: 0,
                authenticated_time_floor: 0,
                qualification_epoch: 0,
                active_profile_digest: [0; 32],
                qualification_record_digest: [0; 32],
            },
            verifying_key_sec1: [0; 65],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RelaySignError {
    Unavailable,
    GenerationMismatch,
    ProfileMismatch,
    QualificationRequired,
    InvalidRequest,
    Failure,
}
