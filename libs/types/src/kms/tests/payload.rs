// SPDX-License-Identifier: MPL-2.0
//! Existing fixed-payload codec round-trip vectors.

use crate::kms::{
    AcquireNodeIdentityPayload, BindingEpoch, BrokerBindingPayload, KmsCapabilityReadiness,
    KmsKeyAlgorithm, KmsProviderKind, NodeIdentityHandle, NodeIdentityState,
    NodeIdentityStatusPayload, RelayP256StatusPayload, RelayProviderAssessment,
    RotateNodeIdentityReason, RotateNodeIdentityRequestPayload, RotateNodeIdentityResponsePayload,
    Tls13ClientCertificateVerifyRequestPayload,
};

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
fn rotation_request_requires_exact_nonzero_revision() {
    let request = RotateNodeIdentityRequestPayload {
        reason: RotateNodeIdentityReason::LostKeyRecovery,
        reserved0: 0,
        flags: 0,
        expected_blob_revision: 9,
    };
    assert_eq!(
        RotateNodeIdentityRequestPayload::decode(&request.encode()),
        Some(request)
    );

    let mut wildcard = request.encode();
    wildcard[8..16].fill(0);
    assert_eq!(RotateNodeIdentityRequestPayload::decode(&wildcard), None);
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

#[test]
fn relay_payload_vectors_are_canonical() {
    let request = Tls13ClientCertificateVerifyRequestPayload {
        transcript_hash: [0x11; 32],
        relay_generation: 0x0807_0605_0403_0201,
        active_profile_digest: [0x22; 32],
        request_id: 0x1817_1615_1413_1211,
    };
    let encoded = request.encode();
    assert_eq!(&encoded[..32], &[0x11; 32]);
    assert_eq!(&encoded[32..40], &[1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(&encoded[40..72], &[0x22; 32]);
    assert_eq!(
        &encoded[72..80],
        &[0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18]
    );
    assert_eq!(
        Tls13ClientCertificateVerifyRequestPayload::decode(&encoded),
        Some(request)
    );

    let status = RelayP256StatusPayload {
        algorithm: KmsKeyAlgorithm::RelayP256Sha256,
        readiness: KmsCapabilityReadiness::Ready,
        provider: KmsProviderKind::HardwareRelay,
        assessment: RelayProviderAssessment::ProductionQualified,
        reserved: 0,
        relay_generation: 7,
        policy_epoch: 8,
        authenticated_time_floor: 9,
        qualification_epoch: 10,
        active_profile_digest: [0x33; 32],
        qualification_record_digest: [0x44; 32],
    };
    assert_eq!(
        RelayP256StatusPayload::decode(&status.encode()),
        Some(status)
    );
    let mut noncanonical = status.encode();
    noncanonical[4] = 1;
    assert_eq!(RelayP256StatusPayload::decode(&noncanonical), None);
}

#[test]
fn stage_profile_request_round_trip_and_bounded_decode() {
    use crate::kms::RelayStageProfileRequestPayload;
    let stage = RelayStageProfileRequestPayload {
        pending_relay_generation: 8,
        expected_policy_epoch: 12,
        profile_digest: [0x77; 32],
    };
    assert_eq!(
        RelayStageProfileRequestPayload::decode(&stage.encode()),
        Some(stage)
    );
    // No padding region exists in this payload: only a wrong length fails.
    assert_eq!(
        RelayStageProfileRequestPayload::decode(&stage.encode()[..47]),
        None
    );
    assert_eq!(RelayStageProfileRequestPayload::decode(&[0u8; 49]), None);
}

#[test]
fn active_public_key_response_round_trip_and_bounded_decode() {
    use crate::kms::RelayActivePublicKeyPayload;
    let key = RelayActivePublicKeyPayload {
        relay_generation: 8,
        spki_sec1: [0x22; 65],
        spki_sha256: [0x88; 32],
    };
    let encoded = key.encode();
    assert_eq!(encoded.len(), 105);
    assert_eq!(RelayActivePublicKeyPayload::decode(&encoded), Some(key));
    assert_eq!(RelayActivePublicKeyPayload::decode(&encoded[..104]), None);
}
