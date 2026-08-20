use crate::storage::{
    assess_for_tests, ActiveSlot, FixtureRootProvider, JournalLoad, JournalRecord, JournalState,
    OpenedRoot, ProviderAssessment, ProviderOpenResult, SlotId,
};
use core::sync::atomic::{AtomicUsize, Ordering};
use types::kms::{BindingEpoch, KmsProviderKind, NodeIdentityState};

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
fn pre_open_policy_gates_skip_provider_calls() {
    static CALLS: AtomicUsize = AtomicUsize::new(0);

    for fixture in [
        FixtureRootProvider {
            kind: KmsProviderKind::HardwareSealed,
            assessment: ProviderAssessment {
                anti_rollback_capable: true,
                current_epoch: 6,
                measurement_ok: true,
                device_binding_ok: true,
                production_capable: false,
            },
            open_result: ProviderOpenResult::Opened(OpenedRoot {
                blob_revision: 6,
                public_key: [2; 32],
            }),
            open_calls: Some(&CALLS),
        },
        FixtureRootProvider {
            kind: KmsProviderKind::TestHooks,
            assessment: ProviderAssessment {
                anti_rollback_capable: true,
                current_epoch: 6,
                measurement_ok: true,
                device_binding_ok: true,
                production_capable: true,
            },
            open_result: ProviderOpenResult::Opened(OpenedRoot {
                blob_revision: 6,
                public_key: [2; 32],
            }),
            open_calls: Some(&CALLS),
        },
        FixtureRootProvider {
            kind: KmsProviderKind::HardwareSealed,
            assessment: ProviderAssessment {
                anti_rollback_capable: true,
                current_epoch: 6,
                measurement_ok: true,
                device_binding_ok: true,
                production_capable: true,
            },
            open_result: ProviderOpenResult::Opened(OpenedRoot {
                blob_revision: 7,
                public_key: [2; 32],
            }),
            open_calls: Some(&CALLS),
        },
    ] {
        CALLS.store(0, Ordering::Relaxed);
        let status = status(fixture, loaded(5, 6, fixture.kind));
        assert_eq!(status.state, NodeIdentityState::PolicyMismatch);
        assert_eq!(status.remote_allowed, 0);
        assert_eq!(status.public_key, [0; 32]);
        assert_eq!(CALLS.load(Ordering::Relaxed), 0);
    }
}
