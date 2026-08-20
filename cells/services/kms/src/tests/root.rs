use crate::storage::{
    assess_for_tests, ActiveSlot, FixtureRootProvider, JournalLoad, JournalRecord, JournalState,
    OpenedRoot, ProviderAssessment, ProviderOpenResult, SlotId,
};
use types::kms::{BindingEpoch, KmsProviderKind, NodeIdentityState, NodeIdentityStatusPayload};

fn status(provider: FixtureRootProvider, journal: JournalState) -> NodeIdentityStatusPayload {
    assess_for_tests(&provider, &journal).status_payload(BindingEpoch(9))
}

fn journal(policy_epoch: u64, blob_revision: u64, provider: KmsProviderKind) -> JournalState {
    JournalState {
        load: JournalLoad::Loaded,
        active: Some(ActiveSlot {
            slot: SlotId::A,
            record: JournalRecord {
                slot: SlotId::A,
                blob_revision,
                policy_epoch,
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
fn unavailable_provider_stays_fail_closed() {
    let status = status(
        FixtureRootProvider {
            kind: KmsProviderKind::None,
            ..FixtureRootProvider::non_production()
        },
        JournalState::empty(),
    );
    assert_eq!(status.state, NodeIdentityState::ProviderUnavailable);
    assert_eq!(status.provider, KmsProviderKind::None);
    assert_eq!(status.remote_allowed, 0);
}

#[test]
fn missing_anti_rollback_reports_root_gate() {
    let status = status(
        FixtureRootProvider {
            kind: KmsProviderKind::SiloWrapped,
            assessment: ProviderAssessment {
                anti_rollback_capable: false,
                current_epoch: 3,
                measurement_ok: true,
                device_binding_ok: true,
                production_capable: true,
            },
            open_result: ProviderOpenResult::Opened(OpenedRoot {
                blob_revision: 8,
                public_key: [9; 32],
            }),
            open_calls: None,
        },
        journal(3, 8, KmsProviderKind::SiloWrapped),
    );
    assert_eq!(status.state, NodeIdentityState::NoAntiRollback);
    assert_eq!(status.remote_allowed, 0);
}

#[test]
fn provider_epoch_older_newer_and_equal_map_correctly() {
    let ready = FixtureRootProvider {
        kind: KmsProviderKind::HardwareSealed,
        assessment: ProviderAssessment {
            anti_rollback_capable: true,
            current_epoch: 7,
            measurement_ok: true,
            device_binding_ok: true,
            production_capable: true,
        },
        open_result: ProviderOpenResult::Opened(OpenedRoot {
            blob_revision: 11,
            public_key: [1; 32],
        }),
        open_calls: None,
    };
    assert_eq!(
        status(ready, journal(7, 11, KmsProviderKind::HardwareSealed)).state,
        NodeIdentityState::Ready
    );
    assert_eq!(
        status(ready, journal(8, 11, KmsProviderKind::HardwareSealed)).state,
        NodeIdentityState::PolicyMismatch
    );
    assert_eq!(
        status(ready, journal(6, 11, KmsProviderKind::HardwareSealed)).state,
        NodeIdentityState::PolicyMismatch
    );
}

#[test]
fn blob_revision_cannot_satisfy_epoch_authority() {
    let status = status(
        FixtureRootProvider {
            kind: KmsProviderKind::DiceSealed,
            assessment: ProviderAssessment {
                anti_rollback_capable: true,
                current_epoch: 2,
                measurement_ok: true,
                device_binding_ok: true,
                production_capable: true,
            },
            open_result: ProviderOpenResult::Opened(OpenedRoot {
                blob_revision: u64::MAX,
                public_key: [2; 32],
            }),
            open_calls: None,
        },
        journal(9, u64::MAX, KmsProviderKind::DiceSealed),
    );
    assert_eq!(status.state, NodeIdentityState::PolicyMismatch);
    assert_eq!(status.remote_allowed, 0);
}

#[test]
fn clone_and_measurement_mismatches_fail_closed() {
    let snapshot = journal(4, 6, KmsProviderKind::DiceSealed);
    let clone = status(
        FixtureRootProvider {
            kind: KmsProviderKind::DiceSealed,
            assessment: ProviderAssessment {
                anti_rollback_capable: true,
                current_epoch: 4,
                measurement_ok: true,
                device_binding_ok: false,
                production_capable: true,
            },
            open_result: ProviderOpenResult::DeviceBindingRejected,
            open_calls: None,
        },
        snapshot.clone(),
    );
    assert_eq!(clone.state, NodeIdentityState::CloneDetected);
    let mismatch = status(
        FixtureRootProvider {
            kind: KmsProviderKind::DiceSealed,
            assessment: ProviderAssessment {
                anti_rollback_capable: true,
                current_epoch: 4,
                measurement_ok: false,
                device_binding_ok: true,
                production_capable: true,
            },
            open_result: ProviderOpenResult::MeasurementMismatch,
            open_calls: None,
        },
        snapshot,
    );
    assert_eq!(mismatch.state, NodeIdentityState::PolicyMismatch);
}

#[test]
fn fixture_ready_mapping_requires_production_capability() {
    let provider = FixtureRootProvider {
        kind: KmsProviderKind::HardwareSealed,
        assessment: ProviderAssessment {
            anti_rollback_capable: true,
            current_epoch: 5,
            measurement_ok: true,
            device_binding_ok: true,
            production_capable: true,
        },
        open_result: ProviderOpenResult::Opened(OpenedRoot {
            blob_revision: 3,
            public_key: [4; 32],
        }),
        open_calls: None,
    };
    let status = status(provider, journal(5, 3, KmsProviderKind::HardwareSealed));
    assert_eq!(status.state, NodeIdentityState::Ready);
    assert_eq!(status.remote_allowed, 1);
    assert_eq!(status.provider, KmsProviderKind::HardwareSealed);
}
