use types::kms::{BindingEpoch, KmsProviderKind, NodeIdentityState, NodeIdentityStatusPayload};

use super::{JournalRecord, JournalState, ProviderOpenResult, RootProvider};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RootAssessment {
    state: NodeIdentityState,
    provider: KmsProviderKind,
    remote_allowed: u8,
    policy_epoch: u64,
    blob_revision: u64,
    public_key: [u8; 32],
}

impl RootAssessment {
    pub(crate) const fn unavailable() -> Self {
        Self::new(
            NodeIdentityState::ProviderUnavailable,
            KmsProviderKind::None,
            0,
            0,
            [0; 32],
        )
    }

    pub(crate) fn from_provider(provider: &impl RootProvider, journal: &JournalState) -> Self {
        let assessment = provider.assess();
        let provider_kind = provider.kind();
        if provider_kind == KmsProviderKind::None {
            return Self::unavailable();
        }
        if provider.seal_epoch().0 != assessment.current_epoch {
            return unavailable_with(provider_kind, journal);
        }
        if !assessment.anti_rollback_capable {
            return limited(
                NodeIdentityState::NoAntiRollback,
                provider_kind,
                journal,
                assessment.current_epoch,
            );
        }
        let journal_epoch = active_policy_epoch(journal);
        if journal_epoch > assessment.current_epoch || !assessment.measurement_ok {
            return limited(
                NodeIdentityState::PolicyMismatch,
                provider_kind,
                journal,
                assessment.current_epoch,
            );
        }
        if !assessment.device_binding_ok {
            return limited(
                NodeIdentityState::CloneDetected,
                provider_kind,
                journal,
                assessment.current_epoch,
            );
        }
        match provider.open_or_provision(active_record(journal)) {
            ProviderOpenResult::Unavailable | ProviderOpenResult::Missing => {
                unavailable_with(provider_kind, journal)
            }
            ProviderOpenResult::DeviceBindingRejected => limited(
                NodeIdentityState::CloneDetected,
                provider_kind,
                journal,
                assessment.current_epoch,
            ),
            ProviderOpenResult::MeasurementMismatch => limited(
                NodeIdentityState::PolicyMismatch,
                provider_kind,
                journal,
                assessment.current_epoch,
            ),
            ProviderOpenResult::Opened(opened)
                if !assessment.production_capable
                    || (journal.active.is_some() && journal_epoch < assessment.current_epoch) =>
            {
                Self::new(
                    NodeIdentityState::PolicyMismatch,
                    provider_kind,
                    opened.blob_revision,
                    assessment.current_epoch,
                    opened.public_key,
                )
            }
            ProviderOpenResult::Opened(opened) => Self::new(
                NodeIdentityState::Ready,
                provider_kind,
                opened.blob_revision,
                assessment.current_epoch,
                opened.public_key,
            ),
        }
    }

    const fn new(
        state: NodeIdentityState,
        provider: KmsProviderKind,
        blob_revision: u64,
        policy_epoch: u64,
        public_key: [u8; 32],
    ) -> Self {
        Self {
            state,
            provider,
            remote_allowed: matches!(state, NodeIdentityState::Ready) as u8,
            policy_epoch,
            blob_revision,
            public_key,
        }
    }

    pub(crate) fn status_payload(self, binding_epoch: BindingEpoch) -> NodeIdentityStatusPayload {
        NodeIdentityStatusPayload {
            state: self.state,
            provider: self.provider,
            remote_allowed: self.remote_allowed,
            reserved: 0,
            binding_epoch,
            blob_revision: self.blob_revision,
            policy_epoch: self.policy_epoch,
            public_key: self.public_key,
        }
    }
}

fn unavailable_with(provider: KmsProviderKind, journal: &JournalState) -> RootAssessment {
    RootAssessment::new(
        NodeIdentityState::ProviderUnavailable,
        provider,
        active_blob_revision(journal),
        0,
        [0; 32],
    )
}

fn limited(
    state: NodeIdentityState,
    provider: KmsProviderKind,
    journal: &JournalState,
    policy_epoch: u64,
) -> RootAssessment {
    RootAssessment::new(
        state,
        provider,
        active_blob_revision(journal),
        policy_epoch,
        [0; 32],
    )
}

fn active_blob_revision(journal: &JournalState) -> u64 {
    journal
        .active
        .as_ref()
        .map_or(0, |active| active.record.blob_revision)
}

fn active_policy_epoch(journal: &JournalState) -> u64 {
    journal
        .active
        .as_ref()
        .map_or(0, |active| active.record.policy_epoch)
}

fn active_record(journal: &JournalState) -> Option<&JournalRecord> {
    journal.active.as_ref().map(|active| &active.record)
}
