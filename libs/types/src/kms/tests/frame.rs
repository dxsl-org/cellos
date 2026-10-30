// SPDX-License-Identifier: MPL-2.0
//! Frame envelope and append-only opcode layout tests.

use crate::kms::{
    BindingEpoch, KmsErrorCode, KmsKeyAlgorithm, KmsOpcode, KmsRequestV1, KmsResponseV1,
    KmsWireError, NodeIdentityHandle, NoiseStaticDhRequestPayload, RelayP256StatusPayload,
    RotateNodeIdentityResponsePayload, ServiceNetBindingPayload, SshAbortRequestPayload,
    SshChunkAckPayload, SshChunkRequestPayload, SshDeleteRequestPayload,
    SshImportBeginRequestPayload, SshImportCommitRequestPayload, SshKeyStatusPayload,
    SshSignRequestPayload, SshSignResponsePayload, SshStatusRequestPayload,
    SshTransferBeginResponsePayload, SshUnlockBeginRequestPayload,
    Tls13ClientCertificateVerifyRequestPayload, Tls13ClientCertificateVerifyResponsePayload,
    KMS_MESSAGE_LEN, KMS_NODE_KEY_ID_C2C, KMS_PAYLOAD_LEN,
};

#[test]
fn fixed_layouts_match_the_frozen_wire_contract() {
    assert_eq!(core::mem::size_of::<KmsRequestV1>(), KMS_MESSAGE_LEN);
    assert_eq!(core::mem::size_of::<KmsResponseV1>(), KMS_MESSAGE_LEN);
    assert_eq!(core::mem::size_of::<NoiseStaticDhRequestPayload>(), 48);
    assert_eq!(
        core::mem::size_of::<RotateNodeIdentityResponsePayload>(),
        48
    );
    assert_eq!(core::mem::size_of::<ServiceNetBindingPayload>(), 32);
    assert_eq!(core::mem::size_of::<RelayP256StatusPayload>(), 104);
    assert_eq!(
        core::mem::size_of::<Tls13ClientCertificateVerifyRequestPayload>(),
        80
    );
    assert_eq!(
        core::mem::size_of::<Tls13ClientCertificateVerifyResponsePayload>(),
        64
    );
    // SSH identity payloads (opcodes 15-24). The 105-byte delete request pads
    // to 112 in memory because it leads with a u64; its wire length is 105.
    assert_eq!(core::mem::size_of::<SshImportBeginRequestPayload>(), 112);
    assert_eq!(core::mem::size_of::<SshTransferBeginResponsePayload>(), 24);
    assert_eq!(core::mem::size_of::<SshChunkRequestPayload>(), 112);
    assert_eq!(core::mem::size_of::<SshChunkAckPayload>(), 16);
    assert_eq!(core::mem::size_of::<SshImportCommitRequestPayload>(), 48);
    assert_eq!(core::mem::size_of::<SshAbortRequestPayload>(), 8);
    assert_eq!(core::mem::size_of::<SshKeyStatusPayload>(), 80);
    assert_eq!(core::mem::size_of::<SshStatusRequestPayload>(), 65);
    assert_eq!(core::mem::size_of::<SshDeleteRequestPayload>(), 112);
    assert_eq!(SshDeleteRequestPayload::LEN, 105);
    assert_eq!(core::mem::size_of::<SshUnlockBeginRequestPayload>(), 48);
    assert_eq!(core::mem::size_of::<SshSignRequestPayload>(), 104);
    assert_eq!(core::mem::size_of::<SshSignResponsePayload>(), 104);
}

#[test]
fn opcode_values_are_append_only() {
    assert_eq!(KmsOpcode::RegisterBrokerInstance as u8, 1);
    assert_eq!(KmsOpcode::GetNodeIdentityStatus as u8, 2);
    assert_eq!(KmsOpcode::AcquireNodeIdentity as u8, 3);
    assert_eq!(KmsOpcode::NoiseStaticDh as u8, 4);
    assert_eq!(KmsOpcode::RotateNodeIdentity as u8, 5);
    assert_eq!(KmsOpcode::RegisterServiceNetInstance as u8, 6);
    assert_eq!(KmsOpcode::GetRelayP256Status as u8, 7);
    assert_eq!(KmsOpcode::SignTls13ClientCertificateVerify as u8, 8);
    assert_eq!(KmsOpcode::BeginRelayEnrollment as u8, 9);
    assert_eq!(KmsOpcode::ReadRelayCsrChunk as u8, 10);
    assert_eq!(KmsOpcode::CommitRelayGeneration as u8, 11);
    assert_eq!(KmsOpcode::AbortRelayEnrollment as u8, 12);
    assert_eq!(KmsOpcode::StageRelayProfile as u8, 13);
    assert_eq!(KmsOpcode::GetRelayActivePublicKey as u8, 14);
    assert_eq!(KmsOpcode::BeginSshKeyImport as u8, 15);
    assert_eq!(KmsOpcode::ImportSshKeyChunk as u8, 16);
    assert_eq!(KmsOpcode::CommitSshKeyImport as u8, 17);
    assert_eq!(KmsOpcode::AbortSshKeyImport as u8, 18);
    assert_eq!(KmsOpcode::GetSshKeyStatus as u8, 19);
    assert_eq!(KmsOpcode::DeleteSshKey as u8, 20);
    assert_eq!(KmsOpcode::SignSshUserauth as u8, 21);
    assert_eq!(KmsOpcode::BeginSshUnlock as u8, 22);
    assert_eq!(KmsOpcode::SshUnlockChunk as u8, 23);
    assert_eq!(KmsOpcode::AbortSshUnlock as u8, 24);
    assert_eq!(KmsOpcode::try_from(25), Err(25));

    assert_eq!(KmsKeyAlgorithm::C2cX25519 as u8, 1);
    assert_eq!(KmsKeyAlgorithm::RelayP256Sha256 as u8, 2);
    assert_eq!(KmsKeyAlgorithm::SshEd25519 as u8, 3);

    assert_eq!(KmsErrorCode::CallerUnattested as u16, 1);
    assert_eq!(KmsErrorCode::PermissionDenied as u16, 2);
    assert_eq!(KmsErrorCode::BindingRequired as u16, 3);
    assert_eq!(KmsErrorCode::BindingStale as u16, 4);
    assert_eq!(KmsErrorCode::SecureRootRequired as u16, 5);
    assert_eq!(KmsErrorCode::CloneDetected as u16, 6);
    assert_eq!(KmsErrorCode::InvalidHandle as u16, 7);
    assert_eq!(KmsErrorCode::InvalidPeerKey as u16, 8);
    assert_eq!(KmsErrorCode::UnknownOpcode as u16, 9);
    assert_eq!(KmsErrorCode::UnsupportedVersion as u16, 10);
    assert_eq!(KmsErrorCode::PersistFailed as u16, 11);
    assert_eq!(KmsErrorCode::ProviderFailure as u16, 12);
    assert_eq!(KmsErrorCode::Busy as u16, 13);
    assert_eq!(KmsErrorCode::ServiceBindingRequired as u16, 14);
    assert_eq!(KmsErrorCode::ServiceBindingStale as u16, 15);
    assert_eq!(KmsErrorCode::RelayUnavailable as u16, 16);
    assert_eq!(KmsErrorCode::RelayGenerationMismatch as u16, 17);
    assert_eq!(KmsErrorCode::ActiveProfileMismatch as u16, 18);
    assert_eq!(KmsErrorCode::InvalidSignature as u16, 19);
    assert_eq!(KmsErrorCode::QualificationRequired as u16, 20);
    assert_eq!(KmsErrorCode::InvalidRequest as u16, 21);

    assert_eq!(KmsErrorCode::EnrollmentPendingExists as u16, 22);
    assert_eq!(KmsErrorCode::CsrHandleInvalid as u16, 23);
    assert_eq!(KmsErrorCode::CsrOrderInvalid as u16, 24);
    assert_eq!(KmsErrorCode::TimeUntrusted as u16, 25);
    assert_eq!(KmsErrorCode::PolicyEpochRegressed as u16, 26);
    assert_eq!(KmsErrorCode::SshKeyUnavailable as u16, 27);
    assert_eq!(KmsErrorCode::SshKeyNotFound as u16, 28);
    assert_eq!(KmsErrorCode::SshKeyExists as u16, 29);
    assert_eq!(KmsErrorCode::SshStoreFull as u16, 30);
    assert_eq!(KmsErrorCode::SshImportPending as u16, 31);
    assert_eq!(KmsErrorCode::SshHandleInvalid as u16, 32);
    assert_eq!(KmsErrorCode::SshOrderInvalid as u16, 33);
    assert_eq!(KmsErrorCode::SshImportIntegrity as u16, 34);
    assert_eq!(KmsErrorCode::SshKeyFormatRejected as u16, 35);
    assert_eq!(KmsErrorCode::SshUnlockRequired as u16, 36);
    assert_eq!(KmsErrorCode::SshUnlockFailed as u16, 37);
    assert_eq!(KmsErrorCode::SshUnlockLocked as u16, 38);
    assert_eq!(KmsErrorCode::try_from(39), Err(39));
}

#[test]
fn request_round_trips_at_the_payload_boundary() {
    let payload = [0xA5; KMS_PAYLOAD_LEN];
    let request = KmsRequestV1::new(KmsOpcode::BeginRelayEnrollment, 42, &payload).unwrap();
    let decoded = KmsRequestV1::from_bytes(&request.to_bytes()).unwrap();
    assert_eq!(decoded.opcode().unwrap(), KmsOpcode::BeginRelayEnrollment);
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
        KmsOpcode::AbortRelayEnrollment,
        8,
        KmsErrorCode::CsrHandleInvalid,
    );
    let error = KmsResponseV1::from_bytes(&error.to_bytes()).unwrap();
    assert_eq!(
        error.error_code().unwrap(),
        Some(KmsErrorCode::CsrHandleInvalid)
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
