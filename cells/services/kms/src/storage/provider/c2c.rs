use types::kms::{KmsProviderKind, RotateNodeIdentityReason};

use super::{
    C2cProvider, JournalRecord, ProviderAssessment, ProviderEpoch, ProviderOpenResult,
};

#[cfg(test)]
use core::sync::atomic::{AtomicUsize, Ordering};

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
impl C2cProvider for FixtureRootProvider {
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
