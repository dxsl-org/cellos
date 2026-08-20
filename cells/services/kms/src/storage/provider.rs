use types::kms::{KmsProviderKind, RotateNodeIdentityReason};

use super::record::JournalRecord;

#[cfg(test)]
use core::sync::atomic::{AtomicUsize, Ordering};

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
pub(crate) trait RootProvider {
    fn kind(&self) -> KmsProviderKind;
    fn assess(&self) -> ProviderAssessment;
    fn open_or_provision(&self, record: Option<&JournalRecord>) -> ProviderOpenResult;
    fn seal_epoch(&self) -> ProviderEpoch;
    fn rotate(
        &self,
        reason: RotateNodeIdentityReason,
        expected_blob_revision: u64,
    ) -> ProviderOpenResult;
}

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct UnavailableRootProvider;

impl RootProvider for UnavailableRootProvider {
    fn kind(&self) -> KmsProviderKind {
        KmsProviderKind::None
    }

    fn assess(&self) -> ProviderAssessment {
        ProviderAssessment::unavailable()
    }

    fn open_or_provision(&self, _record: Option<&JournalRecord>) -> ProviderOpenResult {
        ProviderOpenResult::Unavailable
    }

    fn seal_epoch(&self) -> ProviderEpoch {
        ProviderEpoch(0)
    }

    fn rotate(
        &self,
        _reason: RotateNodeIdentityReason,
        _expected_blob_revision: u64,
    ) -> ProviderOpenResult {
        ProviderOpenResult::Unavailable
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct FixtureRootProvider {
    pub(crate) kind: KmsProviderKind,
    pub(crate) assessment: ProviderAssessment,
    pub(crate) open_result: ProviderOpenResult,
    pub(crate) open_calls: Option<&'static AtomicUsize>,
}

#[cfg(test)]
impl FixtureRootProvider {
    pub(crate) const fn non_production() -> Self {
        Self {
            kind: KmsProviderKind::TestHooks,
            assessment: ProviderAssessment {
                anti_rollback_capable: false,
                current_epoch: 0,
                measurement_ok: true,
                device_binding_ok: true,
                production_capable: false,
            },
            open_result: ProviderOpenResult::Unavailable,
            open_calls: None,
        }
    }
}

#[cfg(test)]
impl RootProvider for FixtureRootProvider {
    fn kind(&self) -> KmsProviderKind {
        self.kind
    }

    fn assess(&self) -> ProviderAssessment {
        self.assessment
    }

    fn open_or_provision(&self, _record: Option<&JournalRecord>) -> ProviderOpenResult {
        if let Some(counter) = self.open_calls {
            counter.fetch_add(1, Ordering::Relaxed);
        }
        self.open_result
    }

    fn seal_epoch(&self) -> ProviderEpoch {
        ProviderEpoch(self.assessment.current_epoch)
    }

    fn rotate(
        &self,
        _reason: RotateNodeIdentityReason,
        _expected_blob_revision: u64,
    ) -> ProviderOpenResult {
        self.open_result
    }
}
