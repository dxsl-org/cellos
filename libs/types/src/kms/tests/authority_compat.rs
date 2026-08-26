// SPDX-License-Identifier: MPL-2.0
//! Golden vectors freezing public KMS opcodes 9-14 wire bytes.

use super::authority_compat_vectors::*;
use crate::kms::{
    KmsErrorCode, KmsOpcode, KmsRequestV1, KmsResponseStatus, KmsResponseV1,
    RelayActivePublicKeyPayload, RelayCsrChunkRequestPayload, RelayCsrChunkResponsePayload,
    RelayEnrollmentAbortRequestPayload, RelayEnrollmentBeginRequestPayload,
    RelayEnrollmentBeginResponsePayload, RelayGenerationCommitRequestPayload,
    RelayGenerationCommitResponsePayload, RelayStageProfileRequestPayload, KMS_MESSAGE_LEN,
};

#[test]
fn opcode_discriminants_nine_through_fourteen_are_frozen() {
    assert_eq!(KmsOpcode::BeginRelayEnrollment as u8, 9);
    assert_eq!(KmsOpcode::ReadRelayCsrChunk as u8, 10);
    assert_eq!(KmsOpcode::CommitRelayGeneration as u8, 11);
    assert_eq!(KmsOpcode::AbortRelayEnrollment as u8, 12);
    assert_eq!(KmsOpcode::StageRelayProfile as u8, 13);
    assert_eq!(KmsOpcode::GetRelayActivePublicKey as u8, 14);
}

#[test]
fn begin_enrollment_vectors_are_frozen() {
    let request = hex::<65>(BEGIN_REQ);
    let parsed = RelayEnrollmentBeginRequestPayload::decode(&request).expect("begin request");
    assert_eq!(parsed.hostname(), b"relay.example.com");
    assert_eq!(parsed.encode(), request);

    let response = hex::<72>(BEGIN_RESP);
    let parsed = RelayEnrollmentBeginResponsePayload::decode(&response).expect("begin response");
    assert_eq!(parsed.pending_relay_generation, 0x0102_0304_0506_0708);
    assert_eq!(parsed.policy_epoch, 7);
    assert_eq!(parsed.restart_epoch, 3);
    assert_eq!(parsed.csr_handle, 0xAABB_CCDD_EEFF_0011);
    assert_eq!(parsed.csr_len, 1000);
    assert_eq!(parsed.csr_sha256, hex::<32>(DIGEST_A));
    assert_eq!(parsed.encode(), response);
}

#[test]
fn csr_chunk_vectors_include_a_short_tail() {
    let request = hex::<16>(CHUNK_REQ);
    let parsed = RelayCsrChunkRequestPayload::decode(&request).expect("chunk request");
    assert_eq!(parsed.csr_handle, 0xAABB_CCDD_EEFF_0011);
    assert_eq!(parsed.chunk_index, 2);
    assert_eq!(parsed.encode(), request);

    for (value, index, length) in [(CHUNK_FULL, 2u32, 104usize), (CHUNK_LAST, 3, 3)] {
        let response = hex::<112>(value);
        let parsed = RelayCsrChunkResponsePayload::decode(&response).expect("chunk response");
        assert_eq!(parsed.chunk_index, index);
        assert_eq!(parsed.chunk_len as usize, length);
        assert_eq!(
            &parsed.chunk[..length.min(4)],
            &hex::<4>("deadbeef")[..length.min(4)]
        );
        assert_eq!(parsed.encode(), response);
    }
}

#[test]
fn commit_abort_and_stage_vectors_are_frozen() {
    let request = hex::<48>(COMMIT_REQ);
    let parsed = RelayGenerationCommitRequestPayload::decode(&request).expect("commit request");
    assert_eq!(parsed.pending_relay_generation, 0x0102_0304_0506_0708);
    assert_eq!(parsed.expected_policy_epoch, 7);
    assert_eq!(parsed.profile_digest, hex::<32>(DIGEST_B));
    assert_eq!(parsed.encode(), request);

    let response = hex::<48>(COMMIT_RESP);
    let parsed = RelayGenerationCommitResponsePayload::decode(&response).expect("commit response");
    assert_eq!(parsed.active_relay_generation, 0x0102_0304_0506_0708);
    assert_eq!(parsed.policy_epoch, 7);
    assert_eq!(parsed.active_profile_digest, hex::<32>(DIGEST_B));
    assert_eq!(parsed.encode(), response);

    let abort = hex::<8>(ABORT_REQ);
    let parsed = RelayEnrollmentAbortRequestPayload::decode(&abort).expect("abort request");
    assert_eq!(parsed.pending_relay_generation, 0x0102_0304_0506_0708);
    assert_eq!(parsed.encode(), abort);

    let stage = hex::<48>(STAGE_REQ);
    let parsed = RelayStageProfileRequestPayload::decode(&stage).expect("stage request");
    assert_eq!(parsed.pending_relay_generation, 9);
    assert_eq!(parsed.expected_policy_epoch, 7);
    assert_eq!(parsed.profile_digest, hex::<32>(DIGEST_A));
    assert_eq!(parsed.encode(), stage);
}

#[test]
fn complete_frames_for_all_public_authority_opcodes_are_frozen() {
    let cases = [
        (
            KmsOpcode::BeginRelayEnrollment,
            65,
            72,
            KmsErrorCode::EnrollmentPendingExists,
            FRAME_BEGIN_REQUEST,
            FRAME_BEGIN_SUCCESS,
            FRAME_BEGIN_ERROR,
        ),
        (
            KmsOpcode::ReadRelayCsrChunk,
            16,
            112,
            KmsErrorCode::CsrOrderInvalid,
            FRAME_CHUNK_REQUEST,
            FRAME_CHUNK_SUCCESS,
            FRAME_CHUNK_ERROR,
        ),
        (
            KmsOpcode::CommitRelayGeneration,
            48,
            48,
            KmsErrorCode::TimeUntrusted,
            FRAME_COMMIT_REQUEST,
            FRAME_COMMIT_SUCCESS,
            FRAME_COMMIT_ERROR,
        ),
        (
            KmsOpcode::AbortRelayEnrollment,
            8,
            0,
            KmsErrorCode::CsrHandleInvalid,
            FRAME_ABORT_REQUEST,
            FRAME_ABORT_SUCCESS,
            FRAME_ABORT_ERROR,
        ),
        (
            KmsOpcode::StageRelayProfile,
            48,
            0,
            KmsErrorCode::PolicyEpochRegressed,
            FRAME_STAGE_REQUEST,
            FRAME_STAGE_SUCCESS,
            FRAME_STAGE_ERROR,
        ),
        (
            KmsOpcode::GetRelayActivePublicKey,
            0,
            105,
            KmsErrorCode::RelayUnavailable,
            FRAME_ACTIVE_KEY_REQUEST,
            FRAME_ACTIVE_KEY_SUCCESS,
            FRAME_ACTIVE_KEY_ERROR,
        ),
    ];

    for (opcode, request_len, success_len, error_code, request, success, error) in cases {
        let request_bytes = hex::<{ KMS_MESSAGE_LEN }>(request);
        let parsed = KmsRequestV1::from_bytes(&request_bytes).expect("literal request frame");
        assert_eq!(parsed.opcode(), Ok(opcode));
        assert_eq!(parsed.request_id, 0x1122_3344);
        assert_eq!(
            parsed.payload().expect("canonical request").len(),
            request_len
        );
        assert_eq!(parsed.to_bytes(), request_bytes);

        let success_bytes = hex::<{ KMS_MESSAGE_LEN }>(success);
        let parsed = KmsResponseV1::from_bytes(&success_bytes).expect("literal success frame");
        assert_eq!(parsed.opcode(), Ok(opcode));
        assert_eq!(parsed.status, KmsResponseStatus::Ok as u8);
        assert_eq!(parsed.request_id, 0x1122_3344);
        assert_eq!(parsed.error_code(), Ok(None));
        assert_eq!(
            parsed.payload().expect("canonical success").len(),
            success_len
        );
        assert_eq!(parsed.to_bytes(), success_bytes);

        let error_bytes = hex::<{ KMS_MESSAGE_LEN }>(error);
        let parsed = KmsResponseV1::from_bytes(&error_bytes).expect("literal error frame");
        assert_eq!(parsed.opcode(), Ok(opcode));
        assert_eq!(parsed.status, KmsResponseStatus::Error as u8);
        assert_eq!(parsed.request_id, 0x1122_3344);
        assert_eq!(parsed.error_code(), Ok(Some(error_code)));
        assert!(parsed.payload().expect("canonical error").is_empty());
        assert_eq!(parsed.to_bytes(), error_bytes);
    }
}

#[test]
fn active_public_key_payload_is_frozen_and_exactly_sized() {
    let payload = hex::<105>(ACTIVE_PUBLIC_KEY);
    let parsed = RelayActivePublicKeyPayload::decode(&payload).expect("active public key");
    assert_eq!(parsed.relay_generation, 0x0102_0304_0506_0708);
    assert_eq!(parsed.spki_sec1[0], 0x04);
    assert_eq!(parsed.spki_sha256, hex::<32>(DIGEST_A));
    assert_eq!(parsed.encode(), payload);
    assert_eq!(RelayActivePublicKeyPayload::LEN, 105);
    let mut smuggled = [0u8; 113];
    smuggled[..105].copy_from_slice(&payload);
    assert_eq!(RelayActivePublicKeyPayload::decode(&smuggled), None);
}
