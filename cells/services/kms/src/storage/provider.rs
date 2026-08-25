#[cfg(test)]
mod c2c;
mod relay;

#[cfg(test)]
pub(crate) use c2c::FixtureRootProvider;
pub(crate) use relay::ProviderSlot;

use types::kms::{
    KmsCapabilityReadiness, KmsKeyAlgorithm, KmsProviderKind, RelayProviderAssessment,
    RotateNodeIdentityReason,
};

use super::record::JournalRecord;
use super::C2cX25519Status;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ProviderEpoch(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProviderAssessment {
    pub(crate) anti_rollback_capable: bool,
    pub(crate) current_epoch: u64,
    pub(crate) measurement_ok: bool,
    pub(crate) device_binding_ok: bool,
    pub(crate) production_capable: bool,
}

impl ProviderAssessment {
    pub(crate) const fn unavailable() -> Self {
        Self {
            anti_rollback_capable: false,
            current_epoch: 0,
            measurement_ok: false,
            device_binding_ok: false,
            production_capable: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OpenedRoot {
    pub(crate) blob_revision: u64,
    pub(crate) public_key: [u8; 32],
}

#[cfg_attr(test, allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderOpenResult {
    Unavailable,
    Missing,
    Opened(OpenedRoot),
    DeviceBindingRejected,
    MeasurementMismatch,
}

#[cfg_attr(test, allow(dead_code))]
pub(crate) trait C2cProvider {
    fn kind(&self) -> KmsProviderKind;
    fn assess(&self) -> ProviderAssessment;
    fn c2c_x25519_status(&self) -> C2cX25519Status {
        let assessment = self.assess();
        let provider = self.kind();
        C2cX25519Status {
            algorithm: KmsKeyAlgorithm::C2cX25519,
            generation: self.seal_epoch().0,
            policy_epoch: assessment.current_epoch,
            provider,
            assessment: if assessment.production_capable {
                RelayProviderAssessment::ProductionQualified
            } else if provider == KmsProviderKind::TestHooks {
                RelayProviderAssessment::DevelopmentReference
            } else {
                RelayProviderAssessment::QualificationTest
            },
            readiness: if provider == KmsProviderKind::None {
                KmsCapabilityReadiness::Unavailable
            } else if assessment.production_capable
                && assessment.measurement_ok
                && assessment.device_binding_ok
            {
                KmsCapabilityReadiness::Ready
            } else {
                KmsCapabilityReadiness::PolicyMismatch
            },
        }
    }

    fn open_or_provision(&self, record: Option<&JournalRecord>) -> ProviderOpenResult;
    fn seal_epoch(&self) -> ProviderEpoch;
    fn rotate(
        &self,
        reason: RotateNodeIdentityReason,
        expected_blob_revision: u64,
    ) -> ProviderOpenResult;
}


