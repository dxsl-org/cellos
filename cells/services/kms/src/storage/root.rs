use types::kms::{BindingEpoch, KmsProviderKind, NodeIdentityState, NodeIdentityStatusPayload};

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
        Self {
            state: NodeIdentityState::ProviderUnavailable,
            provider: KmsProviderKind::None,
            remote_allowed: 0,
            policy_epoch: 0,
            blob_revision: 0,
            public_key: [0; 32],
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
