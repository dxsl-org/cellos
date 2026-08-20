use types::kms::{KmsProviderKind, NodeIdentityState};

use super::{JournalRecord, OpenedRoot};
use crate::storage::root::RootAssessment;

pub(crate) fn ready_or_mismatch(
    provider: KmsProviderKind,
    active: &JournalRecord,
    policy_epoch: u64,
    opened: OpenedRoot,
) -> RootAssessment {
    if active.policy_epoch != policy_epoch
        || opened.blob_revision != active.blob_revision
        || opened.public_key.iter().all(|byte| *byte == 0)
    {
        return RootAssessment::new(
            NodeIdentityState::PolicyMismatch,
            provider,
            active.blob_revision,
            policy_epoch,
            [0; 32],
        );
    }
    RootAssessment::new(
        NodeIdentityState::Ready,
        provider,
        opened.blob_revision,
        policy_epoch,
        opened.public_key,
    )
}
