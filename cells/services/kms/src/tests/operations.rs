use super::*;
use types::kms::{
    NoiseStaticDhRequestPayload, RotateNodeIdentityReason, RotateNodeIdentityRequestPayload,
    KMS_NODE_KEY_ID_C2C,
};

#[test]
fn acquire_requires_binding_then_secure_root() {
    let mut service = KmsService::new();
    let unbound = service
        .handle(
            &request(KmsOpcode::AcquireNodeIdentity, &[]),
            7,
            Some(caller(20, 30, 7)),
            registry(Some(7), Some(8)),
        )
        .unwrap();
    assert_error(unbound, KmsErrorCode::BindingRequired);
    bind(&mut service, 7);
    let bound = service
        .handle(
            &request(KmsOpcode::AcquireNodeIdentity, &[]),
            70,
            Some(caller(20, 30, 70)),
            registry(Some(7), Some(8)),
        )
        .unwrap();
    assert_error(bound, KmsErrorCode::SecureRootRequired);
}

#[test]
fn noise_dh_validates_metadata_before_root_gate() {
    let mut service = KmsService::new();
    let binding = bind(&mut service, 7);
    let mut payload = NoiseStaticDhRequestPayload {
        handle: types::kms::NodeIdentityHandle(1),
        key_id: KMS_NODE_KEY_ID_C2C,
        reserved: 0,
        binding_epoch: binding.binding_epoch,
        peer_public_key: [3; 32],
    };
    let response = service
        .handle(
            &request(KmsOpcode::NoiseStaticDh, &payload.encode()),
            70,
            Some(caller(20, 30, 70)),
            registry(Some(7), Some(8)),
        )
        .unwrap();
    assert_error(response, KmsErrorCode::SecureRootRequired);
    payload.peer_public_key = [0; 32];
    let response = service
        .handle(
            &request(KmsOpcode::NoiseStaticDh, &payload.encode()),
            70,
            Some(caller(20, 30, 70)),
            registry(Some(7), Some(8)),
        )
        .unwrap();
    assert_error(response, KmsErrorCode::InvalidPeerKey);
}

#[test]
fn rotate_is_supervisor_only_and_still_requires_root() {
    let payload = RotateNodeIdentityRequestPayload {
        reason: RotateNodeIdentityReason::OperatorRekey,
        reserved0: 0,
        flags: 0,
        expected_blob_revision: 0,
    };
    let frame = request(KmsOpcode::RotateNodeIdentity, &payload.encode());
    let denied = KmsService::new()
        .handle(
            &frame,
            7,
            Some(caller(20, 30, 7)),
            registry(Some(7), Some(8)),
        )
        .unwrap();
    assert_error(denied, KmsErrorCode::PermissionDenied);
    let allowed = KmsService::new()
        .handle(
            &frame,
            8,
            Some(caller(40, 50, 8)),
            registry(Some(7), Some(8)),
        )
        .unwrap();
    assert_error(allowed, KmsErrorCode::SecureRootRequired);
}
