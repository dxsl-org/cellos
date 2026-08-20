use super::*;
use types::kms::{BindingEpoch, KmsProviderKind, NodeIdentityState, NodeIdentityStatusPayload};

#[test]
fn live_broker_registers_with_nonzero_epoch() {
    let mut service = KmsService::new();
    let binding = bind(&mut service, 7);
    assert_eq!(binding.binding_epoch, BindingEpoch(1));
    assert_eq!(binding.bound_cell_id, 20);
    assert_eq!(binding.bound_generation, 30);
    assert_eq!(binding.bound_service_tid, 7);
}

#[test]
fn register_denies_unattested_zero_generation_and_non_provider() {
    for (identity, live_tid, expected) in [
        (None, Some(7), KmsErrorCode::CallerUnattested),
        (
            Some(caller(20, 0, 7)),
            Some(7),
            KmsErrorCode::CallerUnattested,
        ),
        (
            Some(caller(20, 30, 7)),
            Some(9),
            KmsErrorCode::PermissionDenied,
        ),
    ] {
        let response = KmsService::new()
            .handle(
                &request(KmsOpcode::RegisterBrokerInstance, &[]),
                7,
                identity,
                registry(live_tid, Some(8)),
            )
            .unwrap();
        assert_error(response, expected);
    }
}

#[test]
fn sender_tid_mismatch_is_unattested() {
    let response = KmsService::new()
        .handle(
            &request(KmsOpcode::RegisterBrokerInstance, &[]),
            7,
            Some(caller(20, 30, 70)),
            registry(Some(7), Some(8)),
        )
        .unwrap();
    assert_error(response, KmsErrorCode::CallerUnattested);
}

#[test]
fn broker_replacement_stales_old_binding_and_gets_new_epoch() {
    let mut service = KmsService::new();
    bind(&mut service, 7);
    let stale = service
        .handle(
            &request(KmsOpcode::AcquireNodeIdentity, &[]),
            70,
            Some(caller(20, 30, 70)),
            registry(Some(9), Some(8)),
        )
        .unwrap();
    assert_error(stale, KmsErrorCode::BindingStale);

    let response = service
        .handle(
            &request(KmsOpcode::RegisterBrokerInstance, &[]),
            9,
            Some(caller(21, 31, 9)),
            registry(Some(9), Some(8)),
        )
        .unwrap();
    let replacement = BrokerBindingPayload::decode(response.payload().unwrap()).unwrap();
    assert_eq!(replacement.binding_epoch, BindingEpoch(2));
    assert_eq!(replacement.bound_cell_id, 21);
}

#[test]
fn status_is_fail_closed_for_bound_broker_thread() {
    let mut service = KmsService::new();
    let binding = bind(&mut service, 7);
    let response = service
        .handle(
            &request(KmsOpcode::GetNodeIdentityStatus, &[]),
            70,
            Some(caller(20, 30, 70)),
            registry(Some(7), Some(8)),
        )
        .unwrap();
    let status = NodeIdentityStatusPayload::decode(response.payload().unwrap()).unwrap();
    assert_eq!(status.state, NodeIdentityState::ProviderUnavailable);
    assert_eq!(status.provider, KmsProviderKind::None);
    assert_eq!(status.binding_epoch, binding.binding_epoch);
    assert_eq!(status.remote_allowed, 0);
    assert_eq!(status.public_key, [0; 32]);
}

#[test]
fn live_supervisor_can_read_status_before_binding() {
    let response = KmsService::new()
        .handle(
            &request(KmsOpcode::GetNodeIdentityStatus, &[]),
            8,
            Some(caller(40, 50, 8)),
            registry(None, Some(8)),
        )
        .unwrap();
    assert_eq!(response.error_code().unwrap(), None);
}

#[test]
fn arbitrary_cell_cannot_read_status() {
    let response = KmsService::new()
        .handle(
            &request(KmsOpcode::GetNodeIdentityStatus, &[]),
            9,
            Some(caller(60, 70, 9)),
            registry(Some(7), Some(8)),
        )
        .unwrap();
    assert_error(response, KmsErrorCode::PermissionDenied);
}

#[test]
fn malformed_envelope_is_dropped_without_state_change() {
    let mut service = KmsService::new();
    let mut frame = request(KmsOpcode::RegisterBrokerInstance, &[]);
    frame[0] = 99;
    assert!(service
        .handle(
            &frame,
            7,
            Some(caller(20, 30, 7)),
            registry(Some(7), Some(8))
        )
        .is_none());
    assert_eq!(bind(&mut service, 7).binding_epoch, BindingEpoch(1));
}
