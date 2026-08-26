// SPDX-License-Identifier: MPL-2.0
//! Rejection cases derived from the frozen authority compatibility vectors.

use super::authority_compat_vectors::*;
use crate::kms::{
    KmsRequestV1, KmsResponseV1, KmsWireError, RelayCsrChunkResponsePayload,
    RelayEnrollmentAbortRequestPayload, RelayEnrollmentBeginRequestPayload,
    RelayEnrollmentBeginResponsePayload, RelayGenerationCommitRequestPayload, KMS_MESSAGE_LEN,
};

#[test]
fn malformed_payload_vectors_reject_without_panicking() {
    let mut request = hex::<65>(BEGIN_REQ);
    request[0] = 65;
    assert_eq!(RelayEnrollmentBeginRequestPayload::decode(&request), None);

    request = hex::<65>(BEGIN_REQ);
    request[18] = 1;
    assert_eq!(RelayEnrollmentBeginRequestPayload::decode(&request), None);

    request = hex::<65>(BEGIN_REQ);
    request[1] = b'R';
    assert_eq!(RelayEnrollmentBeginRequestPayload::decode(&request), None);
    assert_eq!(
        RelayEnrollmentBeginRequestPayload::decode(&request[..64]),
        None
    );

    let mut response = hex::<72>(BEGIN_RESP);
    response[36] = 1;
    assert_eq!(RelayEnrollmentBeginResponsePayload::decode(&response), None);

    let mut chunk = hex::<112>(CHUNK_LAST);
    chunk[4] = 0;
    chunk[5] = 0;
    assert_eq!(RelayCsrChunkResponsePayload::decode(&chunk), None);

    chunk = hex::<112>(CHUNK_FULL);
    chunk[4] += 1;
    assert_eq!(RelayCsrChunkResponsePayload::decode(&chunk), None);
    let mut oversized = [0u8; 113];
    oversized[..112].copy_from_slice(&hex::<112>(CHUNK_FULL));
    assert_eq!(RelayCsrChunkResponsePayload::decode(&oversized), None);

    let commit = hex::<48>(COMMIT_REQ);
    assert_eq!(
        RelayGenerationCommitRequestPayload::decode(&commit[..47]),
        None
    );
    assert!(RelayGenerationCommitRequestPayload::decode(&commit).is_some());

    let abort = hex::<8>(ABORT_REQ);
    assert_eq!(
        RelayEnrollmentAbortRequestPayload::decode(&abort[..7]),
        None
    );
    assert!(RelayEnrollmentAbortRequestPayload::decode(&abort).is_some());
}

#[test]
fn literal_full_frames_reject_truncation_and_reserved_bytes() {
    for vector in [
        FRAME_BEGIN_REQUEST,
        FRAME_CHUNK_REQUEST,
        FRAME_COMMIT_REQUEST,
        FRAME_ABORT_REQUEST,
        FRAME_STAGE_REQUEST,
        FRAME_ACTIVE_KEY_REQUEST,
    ] {
        let mut frame = hex::<{ KMS_MESSAGE_LEN }>(vector);
        assert_eq!(
            KmsRequestV1::from_bytes(&frame[..KMS_MESSAGE_LEN - 1]),
            Err(KmsWireError::InvalidLength(KMS_MESSAGE_LEN - 1))
        );
        frame[10] = 1;
        assert_eq!(
            KmsRequestV1::from_bytes(&frame),
            Err(KmsWireError::NonZeroReserved)
        );
    }

    for vector in [
        FRAME_BEGIN_SUCCESS,
        FRAME_BEGIN_ERROR,
        FRAME_CHUNK_SUCCESS,
        FRAME_CHUNK_ERROR,
        FRAME_COMMIT_SUCCESS,
        FRAME_COMMIT_ERROR,
        FRAME_ABORT_SUCCESS,
        FRAME_ABORT_ERROR,
        FRAME_STAGE_SUCCESS,
        FRAME_STAGE_ERROR,
        FRAME_ACTIVE_KEY_SUCCESS,
        FRAME_ACTIVE_KEY_ERROR,
    ] {
        let mut frame = hex::<{ KMS_MESSAGE_LEN }>(vector);
        assert_eq!(
            KmsResponseV1::from_bytes(&frame[..KMS_MESSAGE_LEN - 1]),
            Err(KmsWireError::InvalidLength(KMS_MESSAGE_LEN - 1))
        );
        frame[3] = 1;
        assert_eq!(
            KmsResponseV1::from_bytes(&frame),
            Err(KmsWireError::NonZeroReserved)
        );
    }
}

#[test]
fn literal_error_frames_reject_smuggled_payload_bytes() {
    for vector in [
        FRAME_BEGIN_ERROR,
        FRAME_CHUNK_ERROR,
        FRAME_COMMIT_ERROR,
        FRAME_ABORT_ERROR,
        FRAME_STAGE_ERROR,
        FRAME_ACTIVE_KEY_ERROR,
    ] {
        let mut frame = hex::<{ KMS_MESSAGE_LEN }>(vector);
        frame[KMS_MESSAGE_LEN - 1] = 1;
        assert_eq!(
            KmsResponseV1::from_bytes(&frame),
            Err(KmsWireError::NonCanonicalPayload)
        );
    }
}
