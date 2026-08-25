use types::kms::{BindingEpoch, KmsProviderKind, NodeIdentityState, NodeIdentityStatusPayload};

use super::{
    ready::ready_or_mismatch, C2cProvider, JournalRecord, JournalState, ProviderOpenResult,
};

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

    pub(crate) fn from_provider(provider: &impl C2cProvider, journal: &JournalState) -> Self {
        let assessment = provider.assess();
        let c2c = provider.c2c_x25519_status();
        let provider_kind = c2c.provider;
        if provider_kind == KmsProviderKind::None {
            return Self::unavailable();
        }
        if c2c.generation != assessment.current_epoch
            || c2c.policy_epoch != assessment.current_epoch
            || c2c.algorithm != types::kms::KmsKeyAlgorithm::C2cX25519
        {
            return fail_closed(
                NodeIdentityState::ProviderUnavailable,
                provider_kind,
                journal,
                0,
            );
        }
        if journal.load == super::JournalLoad::RollbackDetected {
            return fail_closed(
                NodeIdentityState::PolicyMismatch,
                provider_kind,
                journal,
                assessment.current_epoch,
            );
        }
        if !assessment.anti_rollback_capable {
            return fail_closed(
                NodeIdentityState::NoAntiRollback,
                provider_kind,
                journal,
                assessment.current_epoch,
            );
        }
        if !assessment.production_capable || provider_kind == KmsProviderKind::TestHooks {
            return fail_closed(
                NodeIdentityState::PolicyMismatch,
                provider_kind,
                journal,
                assessment.current_epoch,
            );
        }
        if !assessment.measurement_ok {
            return fail_closed(
                NodeIdentityState::PolicyMismatch,
                provider_kind,
                journal,
                assessment.current_epoch,
            );
        }
        if !assessment.device_binding_ok {
            return fail_closed(
                NodeIdentityState::CloneDetected,
                provider_kind,
                journal,
                assessment.current_epoch,
            );
        }
        let Some(active) = active_record(journal) else {
            return fail_closed(
                NodeIdentityState::PolicyMismatch,
                provider_kind,
                journal,
                assessment.current_epoch,
            );
        };
        if active.provider != provider_kind {
            return fail_closed(
                NodeIdentityState::BindingInvalid,
                provider_kind,
                journal,
                assessment.current_epoch,
            );
        }
        if active.policy_epoch != assessment.current_epoch {
            return fail_closed(
                NodeIdentityState::PolicyMismatch,
                provider_kind,
                journal,
                assessment.current_epoch,
            );
        }
        match provider.open_or_provision(Some(active)) {
            ProviderOpenResult::Unavailable | ProviderOpenResult::Missing => fail_closed(
                NodeIdentityState::ProviderUnavailable,
                provider_kind,
                journal,
                0,
            ),
            ProviderOpenResult::DeviceBindingRejected => fail_closed(
                NodeIdentityState::CloneDetected,
                provider_kind,
                journal,
                assessment.current_epoch,
            ),
            ProviderOpenResult::MeasurementMismatch => fail_closed(
                NodeIdentityState::PolicyMismatch,
                provider_kind,
                journal,
                assessment.current_epoch,
            ),
            ProviderOpenResult::Opened(opened) => {
                ready_or_mismatch(provider_kind, active, assessment.current_epoch, opened)
            }
        }
    }

    pub(in crate::storage) const fn new(
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

fn fail_closed(
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

fn active_record(journal: &JournalState) -> Option<&JournalRecord> {
    journal.active.as_ref().map(|active| &active.record)
}
