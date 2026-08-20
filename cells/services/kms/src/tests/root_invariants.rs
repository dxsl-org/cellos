use crate::storage::{
    assess_for_tests, ActiveSlot, FixtureRootProvider, JournalLoad, JournalRecord, JournalState,
    OpenedRoot, ProviderAssessment, ProviderOpenResult, SlotId,
};
use types::kms::{BindingEpoch, KmsProviderKind, NodeIdentityState};

fn provider(
    kind: KmsProviderKind,
    epoch: u64,
    open_result: ProviderOpenResult,
) -> FixtureRootProvider {
    FixtureRootProvider {
        kind,
        assessment: ProviderAssessment {
            anti_rollback_capable: true,
            current_epoch: epoch,
            measurement_ok: true,
            device_binding_ok: true,
            production_capable: true,
        },
        open_result,
        open_calls: None,
    }
}

fn status(
    provider: FixtureRootProvider,
    journal: JournalState,
) -> types::kms::NodeIdentityStatusPayload {
    assess_for_tests(&provider, &journal).status_payload(BindingEpoch(9))
}

fn loaded(epoch: u64, revision: u64, provider: KmsProviderKind) -> JournalState {
    JournalState {
        load: JournalLoad::Loaded,
        active: Some(ActiveSlot {
            slot: SlotId::A,
            record: JournalRecord {
                slot: SlotId::A,
                blob_revision: revision,
                policy_epoch: epoch,
                provider,
                public_key: [5; 32],
                payload_len: 4,
                sealed_leaf: [7; 64],
                previous_slot_digest: [0; 32],
            },
        }),
    }
}

#[test]
fn rollback_or_missing_journal_cannot_reach_ready() {
    let opened = ProviderOpenResult::Opened(OpenedRoot {
        blob_revision: 4,
        public_key: [8; 32],
    });
    let empty = status(
        provider(KmsProviderKind::HardwareSealed, 4, opened),
        JournalState::empty(),
    );
    assert_eq!(empty.state, NodeIdentityState::PolicyMismatch);
    let missing = status(
        provider(KmsProviderKind::HardwareSealed, 4, opened),
        JournalState {
            load: JournalLoad::Loaded,
            active: None,
        },
    );
    assert_eq!(missing.state, NodeIdentityState::PolicyMismatch);
    let rollback = status(
        provider(KmsProviderKind::HardwareSealed, 4, opened),
        JournalState {
            load: JournalLoad::RollbackDetected,
            active: Some(
                loaded(4, 4, KmsProviderKind::HardwareSealed)
                    .active
                    .unwrap(),
            ),
        },
    );
    assert_eq!(rollback.state, NodeIdentityState::PolicyMismatch);
}

#[test]
fn provider_kind_mismatch_reports_binding_invalid() {
    let status = status(
        provider(
            KmsProviderKind::HardwareSealed,
            6,
            ProviderOpenResult::Opened(OpenedRoot {
                blob_revision: 2,
                public_key: [3; 32],
            }),
        ),
        loaded(6, 2, KmsProviderKind::DiceSealed),
    );
    assert_eq!(status.state, NodeIdentityState::BindingInvalid);
    assert_eq!(status.remote_allowed, 0);
}

#[test]
fn opened_root_must_match_loaded_revision_and_nonzero_key() {
    let wrong_revision = status(
        provider(
            KmsProviderKind::HardwareSealed,
            7,
            ProviderOpenResult::Opened(OpenedRoot {
                blob_revision: 8,
                public_key: [9; 32],
            }),
        ),
        loaded(7, 7, KmsProviderKind::HardwareSealed),
    );
    assert_eq!(wrong_revision.state, NodeIdentityState::PolicyMismatch);
    let zero_key = status(
        provider(
            KmsProviderKind::HardwareSealed,
            7,
            ProviderOpenResult::Opened(OpenedRoot {
                blob_revision: 7,
                public_key: [0; 32],
            }),
        ),
        loaded(7, 7, KmsProviderKind::HardwareSealed),
    );
    assert_eq!(zero_key.state, NodeIdentityState::PolicyMismatch);
}

#[test]
fn test_hooks_can_never_report_ready() {
    let status = status(
        provider(
            KmsProviderKind::TestHooks,
            5,
            ProviderOpenResult::Opened(OpenedRoot {
                blob_revision: 3,
                public_key: [4; 32],
            }),
        ),
        loaded(5, 3, KmsProviderKind::TestHooks),
    );
    assert_eq!(status.state, NodeIdentityState::PolicyMismatch);
    assert_eq!(status.remote_allowed, 0);
}
