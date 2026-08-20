use super::*;

#[test]
fn fixed_layouts_match_the_frozen_wire_contract() {
    assert_eq!(core::mem::size_of::<KmsRequestV1>(), KMS_MESSAGE_LEN);
    assert_eq!(core::mem::size_of::<KmsResponseV1>(), KMS_MESSAGE_LEN);
    assert_eq!(core::mem::size_of::<NoiseStaticDhRequestPayload>(), 48);
    assert_eq!(
        core::mem::size_of::<RotateNodeIdentityResponsePayload>(),
        48
    );
}

#[test]
fn request_round_trips_at_the_payload_boundary() {
    let payload = [0xA5; KMS_PAYLOAD_LEN];
    let request = KmsRequestV1::new(KmsOpcode::NoiseStaticDh, 42, &payload).unwrap();
    let decoded = KmsRequestV1::from_bytes(&request.to_bytes()).unwrap();
    assert_eq!(decoded.opcode().unwrap(), KmsOpcode::NoiseStaticDh);
    assert_eq!(decoded.request_id, 42);
    assert_eq!(decoded.payload().unwrap(), payload);
}

#[test]
fn response_preserves_success_and_typed_error_shapes() {
    let ok = KmsResponseV1::ok(KmsOpcode::AcquireNodeIdentity, 7, &[1, 2]).unwrap();
    let ok = KmsResponseV1::from_bytes(&ok.to_bytes()).unwrap();
    assert_eq!(ok.error_code().unwrap(), None);
    assert_eq!(ok.payload().unwrap(), [1, 2]);

    let error = KmsResponseV1::error(
        KmsOpcode::AcquireNodeIdentity,
        8,
        KmsErrorCode::SecureRootRequired,
    );
    let error = KmsResponseV1::from_bytes(&error.to_bytes()).unwrap();
    assert_eq!(
        error.error_code().unwrap(),
        Some(KmsErrorCode::SecureRootRequired)
    );
}

#[test]
fn decoder_rejects_unknown_reserved_and_smuggled_bytes() {
    let request = KmsRequestV1::new(KmsOpcode::GetNodeIdentityStatus, 1, &[]).unwrap();

    let mut bytes = request.to_bytes();
    bytes[1] = 0xFF;
    assert_eq!(
        KmsRequestV1::from_bytes(&bytes),
        Err(KmsWireError::UnknownOpcode(0xFF))
    );

    let mut bytes = request.to_bytes();
    bytes[10] = 1;
    assert_eq!(
        KmsRequestV1::from_bytes(&bytes),
        Err(KmsWireError::NonZeroReserved)
    );

    let mut bytes = request.to_bytes();
    bytes[127] = 1;
    assert_eq!(
        KmsRequestV1::from_bytes(&bytes),
        Err(KmsWireError::NonCanonicalPayload)
    );
}

#[test]
fn noise_request_binds_handle_key_slot_epoch_and_peer() {
    let payload = NoiseStaticDhRequestPayload {
        handle: NodeIdentityHandle(9),
        key_id: KMS_NODE_KEY_ID_C2C,
        reserved: 0,
        binding_epoch: BindingEpoch(17),
        peer_public_key: [0x5C; 32],
    };
    assert_eq!(payload.handle.0, 9);
    assert_eq!(payload.binding_epoch.0, 17);
    assert_eq!(payload.peer_public_key, [0x5C; 32]);
}

#[test]
fn typed_payload_codecs_preserve_canonical_bytes() {
    let binding = BrokerBindingPayload {
        binding_epoch: BindingEpoch(3),
        bound_cell_id: 4,
        bound_generation: 5,
        bound_service_tid: 6,
    };
    assert_eq!(
        BrokerBindingPayload::decode(&binding.encode()),
        Some(binding)
    );

    let status = NodeIdentityStatusPayload {
        state: NodeIdentityState::Ready,
        provider: KmsProviderKind::HardwareSealed,
        remote_allowed: 1,
        reserved: 0,
        binding_epoch: BindingEpoch(7),
        blob_revision: 8,
        policy_epoch: 9,
        public_key: [0x11; 32],
    };
    assert_eq!(
        NodeIdentityStatusPayload::decode(&status.encode()),
        Some(status)
    );

    let acquired = AcquireNodeIdentityPayload {
        handle: NodeIdentityHandle(10),
        provider: KmsProviderKind::DiceSealed,
        state: NodeIdentityState::Ready,
        reserved: 0,
        binding_epoch: BindingEpoch(11),
        blob_revision: 12,
        public_key: [0x22; 32],
    };
    assert_eq!(
        AcquireNodeIdentityPayload::decode(&acquired.encode()),
        Some(acquired)
    );

    let rotate = RotateNodeIdentityResponsePayload {
        new_public_key: [0x33; 32],
        blob_revision: 13,
        re_enroll_required: 1,
        reserved: [0; 7],
    };
    assert_eq!(
        RotateNodeIdentityResponsePayload::decode(&rotate.encode()),
        Some(rotate)
    );
}

#[test]
fn status_payload_rejects_noncanonical_alignment_bytes() {
    let status = NodeIdentityStatusPayload {
        state: NodeIdentityState::ProviderUnavailable,
        provider: KmsProviderKind::None,
        remote_allowed: 0,
        reserved: 0,
        binding_epoch: BindingEpoch(1),
        blob_revision: 0,
        policy_epoch: 0,
        public_key: [0; 32],
    };
    let mut encoded = status.encode();
    encoded[4] = 1;
    assert_eq!(NodeIdentityStatusPayload::decode(&encoded), None);
}
