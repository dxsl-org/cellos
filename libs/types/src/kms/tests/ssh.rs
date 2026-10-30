// SPDX-License-Identifier: MPL-2.0
//! Frozen layouts, golden vectors, and strict-decode rejection for the single
//! SSH identity ABI (opcodes 15-24, errors 27-38).
//!
//! Golden bytes were produced by an independent Python `struct` model of the
//! `k2-cp1.md` section 1.3 tables, not by the Rust encoders under test. Frame
//! vectors list the header plus exactly `payload_len` payload bytes; the rest
//! of the 128-byte frame is canonical zero and is checked by exact equality.

use super::authority_compat_vectors::{hex, DIGEST_A};
use crate::kms::{
    ssh_chunk_count, ssh_chunk_len, validate_ssh_key_name, validate_ssh_username,
    KmsCapabilityReadiness, KmsCustodyPosture, KmsErrorCode, KmsKeyAlgorithm, KmsOpcode,
    KmsProviderKind, KmsRequestV1, KmsResponseV1, KmsSshKeyCustody, KmsWireError,
    SshAbortRequestPayload, SshChunkAckPayload, SshChunkRequestPayload, SshDeleteRequestPayload,
    SshImportBeginRequestPayload, SshImportCommitRequestPayload, SshKeyStatusPayload,
    SshSignRequestPayload, SshSignResponsePayload, SshStatusRequestPayload,
    SshTransferBeginResponsePayload, SshUnlockBeginRequestPayload, KMS_MESSAGE_LEN,
    KMS_PAYLOAD_LEN, KMS_SSH_CHUNK_LEN, KMS_SSH_IMPORT_MAX_LEN, KMS_SSH_KEY_NAME_MAX,
    KMS_SSH_PUBLIC_KEY_LEN, KMS_SSH_SECRET_MAX, KMS_SSH_SESSION_ID_LEN, KMS_SSH_SIGNATURE_LEN,
    KMS_SSH_USERNAME_MAX,
};

const FRAME_ID: u32 = 0x1122_3344;
const REQUEST_ID: u64 = 0x1122_3344_5566_7788;
const HANDLE: u64 = 0xAABB_CCDD_EEFF_0011;
const PUBLIC_KEY_HEX: &str = "b33eaef37ea2df7caa010defdea34e241f65f1b529a4f43ed14327f5c54aab62";
const FINGERPRINT_HEX: &str = "5025222ebecf8ecf7014524c0c1c8b81cdcdaed754df8e0e814338e7064f7084";
const SIGN_INPUT_SHA256_HEX: &str =
    "fa3246abe7b89d40c24cd8b330fc0eba243f06d4835cbc88181be36bb4e6de65";
/// RFC 4252 section 7 input for the `k2-cp1.md` section 4.3 vector (147 bytes).
const SIGN_INPUT_HEX: &str = "00000020000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f3200000004757365720000000e7373682d636f6e6e656374696f6e000000097075626c69636b6579010000000b7373682d65643235353139000000330000000b7373682d6564323535313900000020b33eaef37ea2df7caa010defdea34e241f65f1b529a4f43ed14327f5c54aab62";

const IMPORT_BEGIN_REQ: &str = "8877665544332211fa00000009000000a0a1a2a3a4a5a6a7a8a9aaabacadaeafb0b1b2b3b4b5b6b7b8b9babbbcbdbebf6f70732d6b65795f3100000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000";
const TRANSFER_BEGIN_RESP: &str = "1100ffeeddccbbaafa000000030000006000000000000000";
const CHUNK_REQ_LAST: &str = "1100ffeeddccbbaa020000003a0000005c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c0000000000000000000000000000000000000000000000000000000000000000000000000000";
const CHUNK_REQ_FULL: &str = concat!(
    "1100ffeeddccbbaa0100000060000000",
    "5c5c5c5c5c5c5c5c5c5c",
    "5c5c5c5c5c5c5c5c5c5c",
    "5c5c5c5c5c5c5c5c5c5c",
    "5c5c5c5c5c5c5c5c5c5c",
    "5c5c5c5c5c5c5c5c5c5c",
    "5c5c5c5c5c5c5c5c5c5c",
    "5c5c5c5c5c5c5c5c5c5c",
    "5c5c5c5c5c5c5c5c5c5c",
    "5c5c5c5c5c5c5c5c5c5c",
    "5c5c5c5c5c5c",
);
const CHUNK_ACK: &str = "1100ffeeddccbbaa03000000fa000000";
const COMMIT_REQ: &str = "1100ffeeddccbbaa0300000000000000a0a1a2a3a4a5a6a7a8a9aaabacadaeafb0b1b2b3b4b5b6b7b8b9babbbcbdbebf";
const ABORT_REQ: &str = "1100ffeeddccbbaa";
const STATUS_RESP_KEY: &str = "03010001010201000500000000000000b33eaef37ea2df7caa010defdea34e241f65f1b529a4f43ed14327f5c54aab625025222ebecf8ecf7014524c0c1c8b81cdcdaed754df8e0e814338e7064f7084";
const STATUS_RESP_LEAF: &str = "0301000100000100050000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000";
const STATUS_REQ_NAMED: &str = "096f70732d6b65795f3100000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000";
const STATUS_REQ_LEAF: &str = "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000";
const DELETE_REQ: &str = "8877665544332211b33eaef37ea2df7caa010defdea34e241f65f1b529a4f43ed14327f5c54aab62096f70732d6b65795f3100000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000";
const UNLOCK_BEGIN_REQ: &str = "8877665544332211b33eaef37ea2df7caa010defdea34e241f65f1b529a4f43ed14327f5c54aab620800000000000000";
const SIGN_REQ: &str = "8877665544332211b33eaef37ea2df7caa010defdea34e241f65f1b529a4f43ed14327f5c54aab62000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f0475736572000000000000000000000000000000000000000000000000000000";
const SIGN_RESP: &str = "8877665544332211808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9fa0a1a2a3a4a5a6a7a8a9aaabacadaeafb0b1b2b3b4b5b6b7b8b9babbbcbdbebffa3246abe7b89d40c24cd8b330fc0eba243f06d4835cbc88181be36bb4e6de65";

const FRAME_BEGIN_IMPORT_REQUEST: &str = "010f00004433221170000000000000008877665544332211fa00000009000000a0a1a2a3a4a5a6a7a8a9aaabacadaeafb0b1b2b3b4b5b6b7b8b9babbbcbdbebf6f70732d6b65795f3100000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000";
const FRAME_BEGIN_IMPORT_SUCCESS: &str =
    "010f00004433221100001800000000001100ffeeddccbbaafa000000030000006000000000000000";
const FRAME_BEGIN_IMPORT_ERROR: &str = "010f0100443322111d00000000000000";
const FRAME_IMPORT_CHUNK_REQUEST: &str = "011000004433221170000000000000001100ffeeddccbbaa020000003a0000005c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c0000000000000000000000000000000000000000000000000000000000000000000000000000";
const FRAME_IMPORT_CHUNK_SUCCESS: &str =
    "011000004433221100001000000000001100ffeeddccbbaa03000000fa000000";
const FRAME_IMPORT_CHUNK_ERROR: &str = "01100100443322112100000000000000";
const FRAME_COMMIT_IMPORT_REQUEST: &str = "011100004433221130000000000000001100ffeeddccbbaa0300000000000000a0a1a2a3a4a5a6a7a8a9aaabacadaeafb0b1b2b3b4b5b6b7b8b9babbbcbdbebf";
const FRAME_COMMIT_IMPORT_SUCCESS: &str = "0111000044332211000050000000000003010001010201000500000000000000b33eaef37ea2df7caa010defdea34e241f65f1b529a4f43ed14327f5c54aab625025222ebecf8ecf7014524c0c1c8b81cdcdaed754df8e0e814338e7064f7084";
const FRAME_COMMIT_IMPORT_ERROR: &str = "01110100443322112300000000000000";
const FRAME_ABORT_IMPORT_REQUEST: &str = "011200004433221108000000000000001100ffeeddccbbaa";
const FRAME_ABORT_IMPORT_SUCCESS: &str = "01120000443322110000000000000000";
const FRAME_ABORT_IMPORT_ERROR: &str = "01120100443322112000000000000000";
const FRAME_STATUS_REQUEST: &str = "01130000443322114100000000000000096f70732d6b65795f3100000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000";
const FRAME_STATUS_SUCCESS: &str = "0113000044332211000050000000000003010001010201000500000000000000b33eaef37ea2df7caa010defdea34e241f65f1b529a4f43ed14327f5c54aab625025222ebecf8ecf7014524c0c1c8b81cdcdaed754df8e0e814338e7064f7084";
const FRAME_STATUS_ERROR: &str = "01130100443322111c00000000000000";
const FRAME_DELETE_REQUEST: &str = "011400004433221169000000000000008877665544332211b33eaef37ea2df7caa010defdea34e241f65f1b529a4f43ed14327f5c54aab62096f70732d6b65795f3100000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000";
const FRAME_DELETE_SUCCESS: &str = "01140000443322110000000000000000";
const FRAME_DELETE_ERROR: &str = "01140100443322111b00000000000000";
const FRAME_SIGN_REQUEST: &str = "011500004433221168000000000000008877665544332211b33eaef37ea2df7caa010defdea34e241f65f1b529a4f43ed14327f5c54aab62000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f0475736572000000000000000000000000000000000000000000000000000000";
const FRAME_SIGN_SUCCESS: &str = "011500004433221100006800000000008877665544332211808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9fa0a1a2a3a4a5a6a7a8a9aaabacadaeafb0b1b2b3b4b5b6b7b8b9babbbcbdbebffa3246abe7b89d40c24cd8b330fc0eba243f06d4835cbc88181be36bb4e6de65";
const FRAME_SIGN_ERROR: &str = "01150100443322112400000000000000";
const FRAME_UNLOCK_BEGIN_REQUEST: &str = "011600004433221130000000000000008877665544332211b33eaef37ea2df7caa010defdea34e241f65f1b529a4f43ed14327f5c54aab620800000000000000";
const FRAME_UNLOCK_BEGIN_SUCCESS: &str =
    "011600004433221100001800000000001100ffeeddccbbaafa000000030000006000000000000000";
const FRAME_UNLOCK_BEGIN_ERROR: &str = "01160100443322112600000000000000";
const FRAME_UNLOCK_CHUNK_REQUEST: &str = "011700004433221170000000000000001100ffeeddccbbaa01000000600000005c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c";
const FRAME_UNLOCK_CHUNK_SUCCESS: &str =
    "011700004433221100001000000000001100ffeeddccbbaa03000000fa000000";
const FRAME_UNLOCK_CHUNK_ERROR: &str = "01170100443322112500000000000000";
const FRAME_UNLOCK_ABORT_REQUEST: &str = "011800004433221108000000000000001100ffeeddccbbaa";
const FRAME_UNLOCK_ABORT_SUCCESS: &str = "01180000443322110000000000000000";
const FRAME_UNLOCK_ABORT_ERROR: &str = "01180100443322112100000000000000";

fn bytes_of(value: &str) -> Vec<u8> {
    (0..value.len() / 2)
        .map(|index| u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).expect("hex"))
        .collect()
}

fn frame_of(prefix: &str) -> [u8; KMS_MESSAGE_LEN] {
    let prefix = bytes_of(prefix);
    let mut out = [0u8; KMS_MESSAGE_LEN];
    out[..prefix.len()].copy_from_slice(&prefix);
    out
}

fn padded<const N: usize>(bytes: &[u8]) -> [u8; N] {
    let mut out = [0u8; N];
    out[..bytes.len()].copy_from_slice(bytes);
    out
}

fn edited(bytes: &[u8], edits: &[(usize, u8)]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    for (at, value) in edits {
        out[*at] = *value;
    }
    out
}

fn public_key() -> [u8; 32] {
    hex::<32>(PUBLIC_KEY_HEX)
}

fn import_begin() -> SshImportBeginRequestPayload {
    SshImportBeginRequestPayload {
        request_id: REQUEST_ID,
        total_len: 250,
        name_len: 9,
        reserved: [0; 3],
        blob_sha256: hex::<32>(DIGEST_A),
        name: padded(b"ops-key_1"),
    }
}

fn transfer_begin() -> SshTransferBeginResponsePayload {
    SshTransferBeginResponsePayload {
        handle: HANDLE,
        total_len: 250,
        chunk_count: 3,
        chunk_capacity: 96,
        reserved: [0; 6],
    }
}

fn chunk_last() -> SshChunkRequestPayload {
    SshChunkRequestPayload {
        handle: HANDLE,
        chunk_index: 2,
        chunk_len: 58,
        reserved: 0,
        chunk: padded(&[0x5C; 58]),
    }
}

fn chunk_full() -> SshChunkRequestPayload {
    SshChunkRequestPayload {
        handle: HANDLE,
        chunk_index: 1,
        chunk_len: 96,
        reserved: 0,
        chunk: [0x5C; 96],
    }
}

fn chunk_ack() -> SshChunkAckPayload {
    SshChunkAckPayload {
        handle: HANDLE,
        next_chunk_index: 3,
        received_len: 250,
    }
}

fn commit() -> SshImportCommitRequestPayload {
    SshImportCommitRequestPayload {
        handle: HANDLE,
        chunk_count: 3,
        reserved: 0,
        blob_sha256: hex::<32>(DIGEST_A),
    }
}

fn status_with_key() -> SshKeyStatusPayload {
    SshKeyStatusPayload {
        algorithm: KmsKeyAlgorithm::SshEd25519,
        readiness: KmsCapabilityReadiness::Ready,
        provider: KmsProviderKind::None,
        posture: KmsCustodyPosture::DevPostureVolatile,
        key_present: 1,
        custody: KmsSshKeyCustody::PassphraseWrapped,
        key_count: 1,
        store_revision: 5,
        public_key: public_key(),
        fingerprint_sha256: hex::<32>(FINGERPRINT_HEX),
    }
}

fn status_leaf() -> SshKeyStatusPayload {
    SshKeyStatusPayload {
        key_present: 0,
        custody: KmsSshKeyCustody::None,
        public_key: [0; 32],
        fingerprint_sha256: [0; 32],
        ..status_with_key()
    }
}

fn delete_request() -> SshDeleteRequestPayload {
    SshDeleteRequestPayload {
        request_id: REQUEST_ID,
        public_key: public_key(),
        name_len: 9,
        name: padded(b"ops-key_1"),
    }
}

fn unlock_begin() -> SshUnlockBeginRequestPayload {
    SshUnlockBeginRequestPayload {
        request_id: REQUEST_ID,
        public_key: public_key(),
        secret_len: 8,
        reserved0: 0,
        reserved1: 0,
    }
}

fn sign_request() -> SshSignRequestPayload {
    SshSignRequestPayload {
        request_id: REQUEST_ID,
        public_key: public_key(),
        session_id: core::array::from_fn(|index| index as u8),
        username_len: 4,
        username: padded(b"user"),
    }
}

fn sign_response() -> SshSignResponsePayload {
    SshSignResponsePayload {
        request_id: REQUEST_ID,
        signature: core::array::from_fn(|index| 0x80 + index as u8),
        message_sha256: hex::<32>(SIGN_INPUT_SHA256_HEX),
    }
}

#[test]
fn ssh_payload_sizes_are_frozen_and_fit_the_frame_payload() {
    assert_eq!(SshImportBeginRequestPayload::LEN, 112);
    assert_eq!(SshTransferBeginResponsePayload::LEN, 24);
    assert_eq!(SshChunkRequestPayload::LEN, 112);
    assert_eq!(SshChunkAckPayload::LEN, 16);
    assert_eq!(SshImportCommitRequestPayload::LEN, 48);
    assert_eq!(SshAbortRequestPayload::LEN, 8);
    assert_eq!(SshKeyStatusPayload::LEN, 80);
    assert_eq!(SshStatusRequestPayload::LEN, 65);
    assert_eq!(SshDeleteRequestPayload::LEN, 105);
    assert_eq!(SshUnlockBeginRequestPayload::LEN, 48);
    assert_eq!(SshSignRequestPayload::LEN, 104);
    assert_eq!(SshSignResponsePayload::LEN, 104);

    assert_eq!(core::mem::size_of::<SshImportBeginRequestPayload>(), 112);
    assert_eq!(core::mem::size_of::<SshTransferBeginResponsePayload>(), 24);
    assert_eq!(core::mem::size_of::<SshChunkRequestPayload>(), 112);
    assert_eq!(core::mem::size_of::<SshChunkAckPayload>(), 16);
    assert_eq!(core::mem::size_of::<SshImportCommitRequestPayload>(), 48);
    assert_eq!(core::mem::size_of::<SshAbortRequestPayload>(), 8);
    assert_eq!(core::mem::size_of::<SshKeyStatusPayload>(), 80);
    assert_eq!(core::mem::size_of::<SshStatusRequestPayload>(), 65);
    // A leading u64 pads the 105-byte layout to 112 in memory; the wire LEN is 105.
    assert_eq!(core::mem::size_of::<SshDeleteRequestPayload>(), 112);
    assert_eq!(core::mem::size_of::<SshUnlockBeginRequestPayload>(), 48);
    assert_eq!(core::mem::size_of::<SshSignRequestPayload>(), 104);
    assert_eq!(core::mem::size_of::<SshSignResponsePayload>(), 104);

    for len in [
        SshImportBeginRequestPayload::LEN,
        SshTransferBeginResponsePayload::LEN,
        SshChunkRequestPayload::LEN,
        SshChunkAckPayload::LEN,
        SshImportCommitRequestPayload::LEN,
        SshAbortRequestPayload::LEN,
        SshKeyStatusPayload::LEN,
        SshStatusRequestPayload::LEN,
        SshDeleteRequestPayload::LEN,
        SshUnlockBeginRequestPayload::LEN,
        SshSignRequestPayload::LEN,
        SshSignResponsePayload::LEN,
    ] {
        assert!(len <= KMS_PAYLOAD_LEN);
    }
    assert_eq!(core::mem::size_of::<KmsRequestV1>(), KMS_MESSAGE_LEN);
    assert_eq!(core::mem::size_of::<KmsResponseV1>(), KMS_MESSAGE_LEN);
}

#[test]
fn ssh_constants_and_enum_values_are_frozen() {
    assert_eq!(KMS_SSH_KEY_NAME_MAX, 64);
    assert_eq!(KMS_SSH_USERNAME_MAX, 31);
    assert_eq!(KMS_SSH_SESSION_ID_LEN, 32);
    assert_eq!(KMS_SSH_PUBLIC_KEY_LEN, 32);
    assert_eq!(KMS_SSH_SIGNATURE_LEN, 64);
    assert_eq!(KMS_SSH_IMPORT_MAX_LEN, 65_536);
    assert_eq!(KMS_SSH_CHUNK_LEN, 96);
    assert_eq!(KMS_SSH_SECRET_MAX, 1_024);
    // 112-byte payload minus the 16-byte chunk header.
    assert_eq!(KMS_SSH_CHUNK_LEN, KMS_PAYLOAD_LEN - 16);

    assert_eq!(KmsKeyAlgorithm::C2cX25519 as u8, 1);
    assert_eq!(KmsKeyAlgorithm::RelayP256Sha256 as u8, 2);
    assert_eq!(KmsKeyAlgorithm::SshEd25519 as u8, 3);
    assert_eq!(KmsKeyAlgorithm::try_from(4), Err(4));

    assert_eq!(KmsSshKeyCustody::None as u8, 0);
    assert_eq!(KmsSshKeyCustody::Plain as u8, 1);
    assert_eq!(KmsSshKeyCustody::PassphraseWrapped as u8, 2);
    assert_eq!(KmsSshKeyCustody::try_from(3), Err(3));

    assert_eq!(KmsCustodyPosture::Unassessed as u8, 0);
    assert_eq!(KmsCustodyPosture::DevPostureVolatile as u8, 1);
    assert_eq!(KmsCustodyPosture::DevPosturePersistent as u8, 2);
    // No production-qualified value is declared.
    assert_eq!(KmsCustodyPosture::try_from(3), Err(3));
}

#[test]
fn ssh_opcode_and_error_values_are_append_only_and_round_trip() {
    for (opcode, value) in [
        (KmsOpcode::BeginSshKeyImport, 15u8),
        (KmsOpcode::ImportSshKeyChunk, 16),
        (KmsOpcode::CommitSshKeyImport, 17),
        (KmsOpcode::AbortSshKeyImport, 18),
        (KmsOpcode::GetSshKeyStatus, 19),
        (KmsOpcode::DeleteSshKey, 20),
        (KmsOpcode::SignSshUserauth, 21),
        (KmsOpcode::BeginSshUnlock, 22),
        (KmsOpcode::SshUnlockChunk, 23),
        (KmsOpcode::AbortSshUnlock, 24),
    ] {
        assert_eq!(opcode as u8, value);
        assert_eq!(KmsOpcode::try_from(value), Ok(opcode));
    }
    assert_eq!(KmsOpcode::try_from(25), Err(25));
    assert_eq!(KmsOpcode::try_from(0), Err(0));

    for (code, value) in [
        (KmsErrorCode::SshKeyUnavailable, 27u16),
        (KmsErrorCode::SshKeyNotFound, 28),
        (KmsErrorCode::SshKeyExists, 29),
        (KmsErrorCode::SshStoreFull, 30),
        (KmsErrorCode::SshImportPending, 31),
        (KmsErrorCode::SshHandleInvalid, 32),
        (KmsErrorCode::SshOrderInvalid, 33),
        (KmsErrorCode::SshImportIntegrity, 34),
        (KmsErrorCode::SshKeyFormatRejected, 35),
        (KmsErrorCode::SshUnlockRequired, 36),
        (KmsErrorCode::SshUnlockFailed, 37),
        (KmsErrorCode::SshUnlockLocked, 38),
    ] {
        assert_eq!(code as u16, value);
        assert_eq!(KmsErrorCode::try_from(value), Ok(code));
        // A typed error frame carries the code and nothing else.
        let frame = KmsResponseV1::error(KmsOpcode::SignSshUserauth, FRAME_ID, code).to_bytes();
        assert_eq!(&frame[8..10], &value.to_le_bytes());
        assert!(frame[16..].iter().all(|byte| *byte == 0));
        let decoded = KmsResponseV1::from_bytes(&frame).unwrap();
        assert_eq!(decoded.error_code().unwrap(), Some(code));
    }
    assert_eq!(KmsErrorCode::try_from(39), Err(39));
    // A decoder without these codes refuses them fail-closed; here the next
    // unassigned value is refused the same way, at frame decode.
    let mut unknown = KmsResponseV1::error(
        KmsOpcode::SignSshUserauth,
        FRAME_ID,
        KmsErrorCode::SshKeyExists,
    )
    .to_bytes();
    unknown[8] = 39;
    assert_eq!(
        KmsResponseV1::from_bytes(&unknown),
        Err(KmsWireError::UnknownErrorCode(39))
    );
}

#[test]
fn name_and_username_validators_accept_exactly_the_frozen_charsets() {
    for byte in 0..=255u8 {
        let name_ok = byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-';
        let user_ok = name_ok || byte == b'.';
        assert_eq!(validate_ssh_key_name(&[byte]), name_ok, "name byte {byte}");
        assert_eq!(validate_ssh_username(&[byte]), user_ok, "user byte {byte}");
    }
    assert!(!validate_ssh_key_name(b""));
    assert!(validate_ssh_key_name(&[b'a'; 64]));
    assert!(!validate_ssh_key_name(&[b'a'; 65]));
    assert!(!validate_ssh_username(b""));
    assert!(validate_ssh_username(&[b'a'; 31]));
    assert!(!validate_ssh_username(&[b'a'; 32]));
    assert!(validate_ssh_username(b"first.last-1_x"));
    assert!(!validate_ssh_key_name(b"first.last"));
    assert!(!validate_ssh_key_name("k\u{e9}y".as_bytes()));
}

#[test]
fn canonical_chunking_vectors_and_reassembly() {
    for (total, count, last) in [
        (1u32, 1u32, 1u16),
        (96, 1, 96),
        (97, 2, 1),
        (250, 3, 58),
        (290, 4, 2),
        (65_535, 683, 63),
        (65_536, 683, 64),
        (1_024, 11, 64),
    ] {
        assert_eq!(ssh_chunk_count(total), count, "count for {total}");
        assert_eq!(
            ssh_chunk_len(total, count - 1),
            Some(last),
            "last for {total}"
        );
        assert_eq!(ssh_chunk_len(total, count), None);
        for index in 0..count - 1 {
            assert_eq!(ssh_chunk_len(total, index), Some(96));
        }
    }
    assert_eq!(ssh_chunk_count(0), 0);
    assert_eq!(ssh_chunk_len(0, 0), None);
    // 65,536 = 682 * 96 + 64: one Begin, 683 chunks, one Commit.
    assert_eq!(682 * 96 + 64, KMS_SSH_IMPORT_MAX_LEN);

    // Property: every length reassembles to the original and a re-chunked
    // sequence is byte-identical. Sizes: all of 1..=1100 plus a seeded sweep.
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut totals: Vec<u32> = (1..=1100).collect();
    for _ in 0..200 {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        totals.push(1 + ((state >> 33) as u32 % KMS_SSH_IMPORT_MAX_LEN as u32));
    }
    for total in totals {
        let source: Vec<u8> = (0..total).map(|index| (index * 7 + 3) as u8).collect();
        let chunk_all = |source: &[u8]| -> Vec<Vec<u8>> {
            let count = ssh_chunk_count(source.len() as u32);
            let mut offset = 0usize;
            (0..count)
                .map(|index| {
                    let len = ssh_chunk_len(source.len() as u32, index).unwrap() as usize;
                    let chunk = source[offset..offset + len].to_vec();
                    offset += len;
                    chunk
                })
                .collect()
        };
        let chunks = chunk_all(&source);
        assert!(chunks[..chunks.len() - 1].iter().all(|c| c.len() == 96));
        assert!(!chunks.last().unwrap().is_empty() && chunks.last().unwrap().len() <= 96);
        let joined: Vec<u8> = chunks.concat();
        assert_eq!(joined, source);
        assert_eq!(chunk_all(&joined), chunks);
    }
}

#[test]
fn golden_payload_vectors_decode_and_re_encode_exactly() {
    macro_rules! golden {
        ($ty:ty, $vector:expr, $value:expr) => {{
            let bytes = bytes_of($vector);
            assert_eq!(bytes.len(), <$ty>::LEN);
            let value = $value;
            assert_eq!(<$ty>::decode(&bytes), Some(value), stringify!($ty));
            assert_eq!(value.encode().as_slice(), bytes.as_slice(), stringify!($ty));
        }};
    }
    golden!(
        SshImportBeginRequestPayload,
        IMPORT_BEGIN_REQ,
        import_begin()
    );
    golden!(
        SshTransferBeginResponsePayload,
        TRANSFER_BEGIN_RESP,
        transfer_begin()
    );
    golden!(SshChunkRequestPayload, CHUNK_REQ_LAST, chunk_last());
    golden!(SshChunkRequestPayload, CHUNK_REQ_FULL, chunk_full());
    golden!(SshChunkAckPayload, CHUNK_ACK, chunk_ack());
    golden!(SshImportCommitRequestPayload, COMMIT_REQ, commit());
    golden!(
        SshAbortRequestPayload,
        ABORT_REQ,
        SshAbortRequestPayload { handle: HANDLE }
    );
    golden!(SshKeyStatusPayload, STATUS_RESP_KEY, status_with_key());
    golden!(SshKeyStatusPayload, STATUS_RESP_LEAF, status_leaf());
    golden!(
        SshStatusRequestPayload,
        STATUS_REQ_NAMED,
        SshStatusRequestPayload {
            name_len: 9,
            name: padded(b"ops-key_1"),
        }
    );
    golden!(
        SshStatusRequestPayload,
        STATUS_REQ_LEAF,
        SshStatusRequestPayload {
            name_len: 0,
            name: [0; 64],
        }
    );
    golden!(SshDeleteRequestPayload, DELETE_REQ, delete_request());
    golden!(
        SshUnlockBeginRequestPayload,
        UNLOCK_BEGIN_REQ,
        unlock_begin()
    );
    golden!(SshSignRequestPayload, SIGN_REQ, sign_request());
    golden!(SshSignResponsePayload, SIGN_RESP, sign_response());

    assert_eq!(import_begin().name(), b"ops-key_1");
    assert_eq!(delete_request().name(), b"ops-key_1");
    assert_eq!(sign_request().username(), b"user");
    assert_eq!(chunk_last().data(), &[0x5C; 58][..]);
}

type Recode = fn(&[u8]) -> Option<Vec<u8>>;
/// Opcode, golden request/success/error frames, the typed error the error frame
/// carries, and the payload codecs (response codec `None` = empty success).
type FrameCase = (
    KmsOpcode,
    &'static str,
    &'static str,
    &'static str,
    KmsErrorCode,
    Recode,
    Option<Recode>,
);

#[test]
fn golden_frames_for_every_ssh_opcode_are_canonical() {
    let cases: [FrameCase; 10] = [
        (
            KmsOpcode::BeginSshKeyImport,
            FRAME_BEGIN_IMPORT_REQUEST,
            FRAME_BEGIN_IMPORT_SUCCESS,
            FRAME_BEGIN_IMPORT_ERROR,
            KmsErrorCode::SshKeyExists,
            |b| SshImportBeginRequestPayload::decode(b).map(|p| p.encode().to_vec()),
            Some(|b| SshTransferBeginResponsePayload::decode(b).map(|p| p.encode().to_vec())),
        ),
        (
            KmsOpcode::ImportSshKeyChunk,
            FRAME_IMPORT_CHUNK_REQUEST,
            FRAME_IMPORT_CHUNK_SUCCESS,
            FRAME_IMPORT_CHUNK_ERROR,
            KmsErrorCode::SshOrderInvalid,
            |b| SshChunkRequestPayload::decode(b).map(|p| p.encode().to_vec()),
            Some(|b| SshChunkAckPayload::decode(b).map(|p| p.encode().to_vec())),
        ),
        (
            KmsOpcode::CommitSshKeyImport,
            FRAME_COMMIT_IMPORT_REQUEST,
            FRAME_COMMIT_IMPORT_SUCCESS,
            FRAME_COMMIT_IMPORT_ERROR,
            KmsErrorCode::SshKeyFormatRejected,
            |b| SshImportCommitRequestPayload::decode(b).map(|p| p.encode().to_vec()),
            Some(|b| SshKeyStatusPayload::decode(b).map(|p| p.encode().to_vec())),
        ),
        (
            KmsOpcode::AbortSshKeyImport,
            FRAME_ABORT_IMPORT_REQUEST,
            FRAME_ABORT_IMPORT_SUCCESS,
            FRAME_ABORT_IMPORT_ERROR,
            KmsErrorCode::SshHandleInvalid,
            |b| SshAbortRequestPayload::decode(b).map(|p| p.encode().to_vec()),
            None,
        ),
        (
            KmsOpcode::GetSshKeyStatus,
            FRAME_STATUS_REQUEST,
            FRAME_STATUS_SUCCESS,
            FRAME_STATUS_ERROR,
            KmsErrorCode::SshKeyNotFound,
            |b| SshStatusRequestPayload::decode(b).map(|p| p.encode().to_vec()),
            Some(|b| SshKeyStatusPayload::decode(b).map(|p| p.encode().to_vec())),
        ),
        (
            KmsOpcode::DeleteSshKey,
            FRAME_DELETE_REQUEST,
            FRAME_DELETE_SUCCESS,
            FRAME_DELETE_ERROR,
            KmsErrorCode::SshKeyUnavailable,
            |b| SshDeleteRequestPayload::decode(b).map(|p| p.encode().to_vec()),
            None,
        ),
        (
            KmsOpcode::SignSshUserauth,
            FRAME_SIGN_REQUEST,
            FRAME_SIGN_SUCCESS,
            FRAME_SIGN_ERROR,
            KmsErrorCode::SshUnlockRequired,
            |b| SshSignRequestPayload::decode(b).map(|p| p.encode().to_vec()),
            Some(|b| SshSignResponsePayload::decode(b).map(|p| p.encode().to_vec())),
        ),
        (
            KmsOpcode::BeginSshUnlock,
            FRAME_UNLOCK_BEGIN_REQUEST,
            FRAME_UNLOCK_BEGIN_SUCCESS,
            FRAME_UNLOCK_BEGIN_ERROR,
            KmsErrorCode::SshUnlockLocked,
            |b| SshUnlockBeginRequestPayload::decode(b).map(|p| p.encode().to_vec()),
            Some(|b| SshTransferBeginResponsePayload::decode(b).map(|p| p.encode().to_vec())),
        ),
        (
            KmsOpcode::SshUnlockChunk,
            FRAME_UNLOCK_CHUNK_REQUEST,
            FRAME_UNLOCK_CHUNK_SUCCESS,
            FRAME_UNLOCK_CHUNK_ERROR,
            KmsErrorCode::SshUnlockFailed,
            |b| SshChunkRequestPayload::decode(b).map(|p| p.encode().to_vec()),
            Some(|b| SshChunkAckPayload::decode(b).map(|p| p.encode().to_vec())),
        ),
        (
            KmsOpcode::AbortSshUnlock,
            FRAME_UNLOCK_ABORT_REQUEST,
            FRAME_UNLOCK_ABORT_SUCCESS,
            FRAME_UNLOCK_ABORT_ERROR,
            KmsErrorCode::SshOrderInvalid,
            |b| SshAbortRequestPayload::decode(b).map(|p| p.encode().to_vec()),
            None,
        ),
    ];
    for (opcode, request, success, error, code, request_codec, response_codec) in cases {
        // Request.
        let wire = frame_of(request);
        let decoded = KmsRequestV1::from_bytes(&wire).unwrap();
        assert_eq!(decoded.opcode().unwrap(), opcode);
        assert_eq!(decoded.request_id, FRAME_ID);
        let payload = decoded.payload().unwrap();
        assert_eq!(request_codec(payload).as_deref(), Some(payload));
        assert_eq!(
            KmsRequestV1::new(opcode, FRAME_ID, payload)
                .unwrap()
                .to_bytes(),
            wire
        );
        // Success.
        let wire = frame_of(success);
        let decoded = KmsResponseV1::from_bytes(&wire).unwrap();
        assert_eq!(decoded.opcode().unwrap(), opcode);
        assert_eq!(decoded.error_code().unwrap(), None);
        let payload = decoded.payload().unwrap();
        match response_codec {
            Some(codec) => assert_eq!(codec(payload).as_deref(), Some(payload)),
            None => assert!(payload.is_empty()),
        }
        assert_eq!(
            KmsResponseV1::ok(opcode, FRAME_ID, payload)
                .unwrap()
                .to_bytes(),
            wire
        );
        // Typed error with an empty payload.
        let wire = frame_of(error);
        let decoded = KmsResponseV1::from_bytes(&wire).unwrap();
        assert_eq!(decoded.error_code().unwrap(), Some(code));
        assert_eq!(
            code as u16,
            u16::from_le_bytes([frame_of(error)[8], frame_of(error)[9]])
        );
        assert!(decoded.payload().unwrap().is_empty());
        assert_eq!(
            KmsResponseV1::error(opcode, FRAME_ID, code).to_bytes(),
            wire
        );
    }
}

#[test]
fn ssh_opcodes_reject_noncanonical_envelopes_without_a_typed_reply() {
    for opcode in [
        KmsOpcode::BeginSshKeyImport,
        KmsOpcode::SignSshUserauth,
        KmsOpcode::AbortSshUnlock,
    ] {
        let request = KmsRequestV1::new(opcode, FRAME_ID, &[]).unwrap().to_bytes();
        for (at, error) in [
            (2, KmsWireError::NonZeroReserved),
            (3, KmsWireError::NonZeroReserved),
            (10, KmsWireError::NonZeroReserved),
            (15, KmsWireError::NonZeroReserved),
            (127, KmsWireError::NonCanonicalPayload),
        ] {
            let mut bytes = request;
            bytes[at] = 1;
            assert_eq!(KmsRequestV1::from_bytes(&bytes), Err(error), "byte {at}");
        }
        let mut bytes = request;
        bytes[0] = 2;
        assert_eq!(
            KmsRequestV1::from_bytes(&bytes),
            Err(KmsWireError::UnsupportedVersion(2))
        );
        let mut bytes = request;
        bytes[1] = 25;
        assert_eq!(
            KmsRequestV1::from_bytes(&bytes),
            Err(KmsWireError::UnknownOpcode(25))
        );
        let mut bytes = request;
        bytes[8] = 113;
        assert_eq!(
            KmsRequestV1::from_bytes(&bytes),
            Err(KmsWireError::PayloadTooLong(113))
        );
    }
}

#[test]
fn import_begin_decode_rejects_every_noncanonical_shape() {
    let good = bytes_of(IMPORT_BEGIN_REQ);
    assert!(SshImportBeginRequestPayload::decode(&good).is_some());
    assert!(SshImportBeginRequestPayload::decode(&good[..111]).is_none());
    assert!(SshImportBeginRequestPayload::decode(&[good.clone(), vec![0]].concat()).is_none());
    for at in 13..16 {
        assert!(SshImportBeginRequestPayload::decode(&edited(&good, &[(at, 1)])).is_none());
    }
    // request_id 0.
    assert!(SshImportBeginRequestPayload::decode(&edited(
        &good,
        &[
            (0, 0),
            (1, 0),
            (2, 0),
            (3, 0),
            (4, 0),
            (5, 0),
            (6, 0),
            (7, 0)
        ]
    ))
    .is_none());
    // total_len bounds.
    for (total, ok) in [
        (0u32, false),
        (1, true),
        (65_536, true),
        (65_537, false),
        (u32::MAX, false),
    ] {
        let mut bytes = good.clone();
        bytes[8..12].copy_from_slice(&total.to_le_bytes());
        assert_eq!(
            SshImportBeginRequestPayload::decode(&bytes).is_some(),
            ok,
            "{total}"
        );
    }
    // name_len bounds: 0 and 65 are rejected, 64 with a 64-byte name accepted.
    for len in [0u8, 65, 255] {
        assert!(SshImportBeginRequestPayload::decode(&edited(&good, &[(12, len)])).is_none());
    }
    let mut max_name = edited(&good, &[(12, 64)]);
    max_name[48..112].fill(b'k');
    assert!(SshImportBeginRequestPayload::decode(&max_name).is_some());
    // Nonzero padding after the declared name.
    assert!(SshImportBeginRequestPayload::decode(&edited(&good, &[(57, 1)])).is_none());
    assert!(SshImportBeginRequestPayload::decode(&edited(&good, &[(111, 1)])).is_none());
    // Charset.
    for bad in [b' ', b'.', b'/', 0x00, 0x7F, 0x80, 0xFF] {
        assert!(SshImportBeginRequestPayload::decode(&edited(&good, &[(50, bad)])).is_none());
    }
    // All-zero digest.
    let mut zero_digest = good.clone();
    zero_digest[16..48].fill(0);
    assert!(SshImportBeginRequestPayload::decode(&zero_digest).is_none());
}

#[test]
fn transfer_begin_response_decode_checks_the_canonical_chunk_plan() {
    let good = bytes_of(TRANSFER_BEGIN_RESP);
    assert!(SshTransferBeginResponsePayload::decode(&good[..23]).is_none());
    for at in 18..24 {
        assert!(SshTransferBeginResponsePayload::decode(&edited(&good, &[(at, 1)])).is_none());
    }
    assert!(SshTransferBeginResponsePayload::decode(&edited(
        &good,
        &[
            (0, 0),
            (1, 0),
            (2, 0),
            (3, 0),
            (4, 0),
            (5, 0),
            (6, 0),
            (7, 0)
        ]
    ))
    .is_none());
    for (total, count, capacity, ok) in [
        (250u32, 3u32, 96u16, true),
        (1_024, 11, 96, true),
        (65_536, 683, 96, true),
        (1, 1, 96, true),
        (250, 2, 96, false),
        (250, 4, 96, false),
        (250, 3, 95, false),
        (250, 3, 97, false),
        (0, 0, 96, false),
        (65_537, 684, 96, false),
    ] {
        let mut bytes = good.clone();
        bytes[8..12].copy_from_slice(&total.to_le_bytes());
        bytes[12..16].copy_from_slice(&count.to_le_bytes());
        bytes[16..18].copy_from_slice(&capacity.to_le_bytes());
        assert_eq!(
            SshTransferBeginResponsePayload::decode(&bytes).is_some(),
            ok,
            "{total}/{count}/{capacity}"
        );
    }
}

#[test]
fn chunk_request_decode_rejects_bad_lengths_reserved_and_padding() {
    let good = bytes_of(CHUNK_REQ_LAST);
    assert!(SshChunkRequestPayload::decode(&good[..111]).is_none());
    for at in 14..16 {
        assert!(SshChunkRequestPayload::decode(&edited(&good, &[(at, 1)])).is_none());
    }
    // chunk_len bounds with canonical zero padding rebuilt for each length.
    for (len, ok) in [
        (0u16, false),
        (1, true),
        (58, true),
        (96, true),
        (97, false),
        (65_535, false),
    ] {
        let mut bytes = vec![0u8; 112];
        bytes[..8].copy_from_slice(&HANDLE.to_le_bytes());
        bytes[12..14].copy_from_slice(&len.to_le_bytes());
        if (len as usize) <= 96 {
            bytes[16..16 + len as usize].fill(0x5C);
        }
        assert_eq!(
            SshChunkRequestPayload::decode(&bytes).is_some(),
            ok,
            "{len}"
        );
    }
    // A byte after chunk_len must be zero.
    assert!(SshChunkRequestPayload::decode(&edited(&good, &[(16 + 58, 1)])).is_none());
    assert!(SshChunkRequestPayload::decode(&edited(&good, &[(111, 1)])).is_none());
    // A zero handle is structurally valid here; the service answers `SshHandleInvalid`.
    assert!(SshChunkRequestPayload::decode(&edited(
        &good,
        &[
            (0, 0),
            (1, 0),
            (2, 0),
            (3, 0),
            (4, 0),
            (5, 0),
            (6, 0),
            (7, 0)
        ]
    ))
    .is_some());
}

#[test]
fn chunk_ack_decode_requires_the_canonical_next_index() {
    let good = bytes_of(CHUNK_ACK);
    assert!(SshChunkAckPayload::decode(&good[..15]).is_none());
    for (handle, next, received, ok) in [
        (HANDLE, 3u32, 250u32, true),
        (HANDLE, 1, 1, true),
        (HANDLE, 1, 96, true),
        (HANDLE, 2, 97, true),
        (HANDLE, 683, 65_536, true),
        (0, 3, 250, false),
        (HANDLE, 2, 250, false),
        (HANDLE, 4, 250, false),
        (HANDLE, 0, 0, false),
        (HANDLE, 684, 65_537, false),
    ] {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&handle.to_le_bytes());
        bytes.extend_from_slice(&next.to_le_bytes());
        bytes.extend_from_slice(&received.to_le_bytes());
        assert_eq!(
            SshChunkAckPayload::decode(&bytes).is_some(),
            ok,
            "{handle}/{next}/{received}"
        );
    }
}

#[test]
fn commit_and_abort_decode_check_only_structure() {
    let good = bytes_of(COMMIT_REQ);
    assert!(SshImportCommitRequestPayload::decode(&good[..47]).is_none());
    for at in 12..16 {
        assert!(SshImportCommitRequestPayload::decode(&edited(&good, &[(at, 1)])).is_none());
    }
    // Count and digest mismatches are the service's order/integrity errors,
    // so the codec accepts any count and any digest.
    let mut other = good.clone();
    other[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    other[16..48].fill(0);
    assert!(SshImportCommitRequestPayload::decode(&other).is_some());

    let abort = bytes_of(ABORT_REQ);
    assert!(SshAbortRequestPayload::decode(&abort).is_some());
    assert!(SshAbortRequestPayload::decode(&abort[..7]).is_none());
    assert!(SshAbortRequestPayload::decode(&[abort, vec![0]].concat()).is_none());
}

#[test]
fn key_status_decode_enforces_presence_invariants_and_known_enums() {
    let key = bytes_of(STATUS_RESP_KEY);
    let leaf = bytes_of(STATUS_RESP_LEAF);
    assert!(SshKeyStatusPayload::decode(&key[..79]).is_none());
    assert!(SshKeyStatusPayload::decode(&[key.clone(), vec![0]].concat()).is_none());

    // Algorithm must be the SSH algorithm.
    for algorithm in [0u8, 1, 2, 4, 255] {
        assert!(SshKeyStatusPayload::decode(&edited(&key, &[(0, algorithm)])).is_none());
    }
    // Every defined readiness, provider, posture, and custody value is accepted;
    // the next undefined value is not.
    for readiness in 0..=4u8 {
        assert!(SshKeyStatusPayload::decode(&edited(&key, &[(1, readiness)])).is_some());
    }
    for provider in 0..=5u8 {
        assert!(SshKeyStatusPayload::decode(&edited(&key, &[(2, provider)])).is_some());
    }
    for posture in 0..=2u8 {
        assert!(SshKeyStatusPayload::decode(&edited(&key, &[(3, posture)])).is_some());
    }
    for (at, value) in [(1, 5u8), (2, 6), (3, 3)] {
        assert!(SshKeyStatusPayload::decode(&edited(&key, &[(at, value)])).is_none());
    }
    assert!(SshKeyStatusPayload::decode(&edited(&key, &[(5, 1)])).is_some());
    assert!(SshKeyStatusPayload::decode(&edited(&key, &[(5, 3)])).is_none());

    // key_present is 0 or 1; a present key has a custody and exactly one key.
    assert!(SshKeyStatusPayload::decode(&edited(&key, &[(4, 2)])).is_none());
    assert!(SshKeyStatusPayload::decode(&edited(&key, &[(5, 0)])).is_none());
    assert!(SshKeyStatusPayload::decode(&edited(&key, &[(6, 0)])).is_none());
    assert!(SshKeyStatusPayload::decode(&edited(&key, &[(6, 2)])).is_none());

    // The leaf query: no key means no custody, public key, or fingerprint; the
    // store may still hold one key.
    assert!(SshKeyStatusPayload::decode(&leaf).is_some());
    assert!(SshKeyStatusPayload::decode(&edited(&leaf, &[(6, 0)])).is_some());
    assert!(SshKeyStatusPayload::decode(&edited(&leaf, &[(6, 2)])).is_none());
    assert!(SshKeyStatusPayload::decode(&edited(&leaf, &[(5, 1)])).is_none());
    assert!(SshKeyStatusPayload::decode(&edited(&leaf, &[(16, 1)])).is_none());
    assert!(SshKeyStatusPayload::decode(&edited(&leaf, &[(79, 1)])).is_none());
}

#[test]
fn status_request_allows_the_empty_leaf_query_only_when_fully_zero() {
    let named = bytes_of(STATUS_REQ_NAMED);
    let leaf = bytes_of(STATUS_REQ_LEAF);
    assert!(SshStatusRequestPayload::decode(&named).is_some());
    assert!(SshStatusRequestPayload::decode(&leaf).is_some());
    assert!(SshStatusRequestPayload::decode(&leaf[..64]).is_none());
    // name_len 0 with a nonzero byte, and 65 or more, are rejected.
    assert!(SshStatusRequestPayload::decode(&edited(&leaf, &[(1, b'a')])).is_none());
    assert!(SshStatusRequestPayload::decode(&edited(&leaf, &[(64, 1)])).is_none());
    for len in [65u8, 255] {
        assert!(SshStatusRequestPayload::decode(&edited(&named, &[(0, len)])).is_none());
    }
    // Nonzero padding and charset violations.
    assert!(SshStatusRequestPayload::decode(&edited(&named, &[(10, 1)])).is_none());
    assert!(SshStatusRequestPayload::decode(&edited(&named, &[(2, b'.')])).is_none());
    let mut max_name = edited(&named, &[(0, 64)]);
    max_name[1..].fill(b'k');
    assert!(SshStatusRequestPayload::decode(&max_name).is_some());
}

#[test]
fn delete_request_decode_rejects_zero_id_bad_names_and_padding() {
    let good = bytes_of(DELETE_REQ);
    assert_eq!(good.len(), 105);
    assert!(SshDeleteRequestPayload::decode(&good).is_some());
    assert!(SshDeleteRequestPayload::decode(&good[..104]).is_none());
    assert!(SshDeleteRequestPayload::decode(&[good.clone(), vec![0]].concat()).is_none());
    assert!(SshDeleteRequestPayload::decode(&edited(
        &good,
        &[
            (0, 0),
            (1, 0),
            (2, 0),
            (3, 0),
            (4, 0),
            (5, 0),
            (6, 0),
            (7, 0)
        ]
    ))
    .is_none());
    for len in [0u8, 65, 255] {
        assert!(SshDeleteRequestPayload::decode(&edited(&good, &[(40, len)])).is_none());
    }
    assert!(SshDeleteRequestPayload::decode(&edited(&good, &[(41 + 9, 1)])).is_none());
    assert!(SshDeleteRequestPayload::decode(&edited(&good, &[(42, b' ')])).is_none());
    // Any 32-byte public key is structurally valid; a mismatch is service state.
    assert!(SshDeleteRequestPayload::decode(&edited(&good, &[(8, 0), (9, 0)])).is_some());
}

#[test]
fn unlock_begin_decode_bounds_the_secret_and_zeroes_reserved() {
    let good = bytes_of(UNLOCK_BEGIN_REQ);
    assert!(SshUnlockBeginRequestPayload::decode(&good[..47]).is_none());
    for at in 42..48 {
        assert!(SshUnlockBeginRequestPayload::decode(&edited(&good, &[(at, 1)])).is_none());
    }
    assert!(SshUnlockBeginRequestPayload::decode(&edited(
        &good,
        &[
            (0, 0),
            (1, 0),
            (2, 0),
            (3, 0),
            (4, 0),
            (5, 0),
            (6, 0),
            (7, 0)
        ]
    ))
    .is_none());
    for (len, ok) in [
        (0u16, false),
        (1, true),
        (1_024, true),
        (1_025, false),
        (65_535, false),
    ] {
        let mut bytes = good.clone();
        bytes[40..42].copy_from_slice(&len.to_le_bytes());
        assert_eq!(
            SshUnlockBeginRequestPayload::decode(&bytes).is_some(),
            ok,
            "{len}"
        );
    }
}

#[test]
fn sign_request_decode_rejects_zero_ids_bad_users_and_padding() {
    let good = bytes_of(SIGN_REQ);
    assert_eq!(good.len(), 104);
    assert!(SshSignRequestPayload::decode(&good[..103]).is_none());
    assert!(SshSignRequestPayload::decode(&[good.clone(), vec![0]].concat()).is_none());
    assert!(SshSignRequestPayload::decode(&edited(
        &good,
        &[
            (0, 0),
            (1, 0),
            (2, 0),
            (3, 0),
            (4, 0),
            (5, 0),
            (6, 0),
            (7, 0)
        ]
    ))
    .is_none());
    let mut zero_session = good.clone();
    zero_session[40..72].fill(0);
    assert!(SshSignRequestPayload::decode(&zero_session).is_none());
    for len in [0u8, 32, 255] {
        assert!(SshSignRequestPayload::decode(&edited(&good, &[(72, len)])).is_none());
    }
    assert!(SshSignRequestPayload::decode(&edited(&good, &[(73 + 4, 1)])).is_none());
    assert!(SshSignRequestPayload::decode(&edited(&good, &[(103, 1)])).is_none());
    for bad in [b'@', b' ', b'/', b'\\', 0x00, 0x80] {
        assert!(SshSignRequestPayload::decode(&edited(&good, &[(74, bad)])).is_none());
    }
    // '.', '_' and '-' are valid username bytes; 31 bytes is the bound.
    assert!(SshSignRequestPayload::decode(&edited(&good, &[(74, b'.')])).is_some());
    let mut max_user = edited(&good, &[(72, 31)]);
    max_user[73..104].fill(b'u');
    assert!(SshSignRequestPayload::decode(&max_user).is_some());
    // The 8-byte request id is free of floor semantics here; any nonzero value decodes.
    assert!(SshSignRequestPayload::decode(&edited(&good, &[(7, 0x80)])).is_some());
}

#[test]
fn sign_response_decode_requires_an_echoed_nonzero_request_id() {
    let good = bytes_of(SIGN_RESP);
    assert!(SshSignResponsePayload::decode(&good).is_some());
    assert!(SshSignResponsePayload::decode(&good[..103]).is_none());
    assert!(SshSignResponsePayload::decode(&[good.clone(), vec![0]].concat()).is_none());
    assert!(SshSignResponsePayload::decode(&edited(
        &good,
        &[
            (0, 0),
            (1, 0),
            (2, 0),
            (3, 0),
            (4, 0),
            (5, 0),
            (6, 0),
            (7, 0)
        ]
    ))
    .is_none());
}

#[test]
fn signing_input_sizing_proof_holds_for_every_username_length() {
    // RFC 4252 section 7 as reconstructed by KMS (k2-cp1.md section 1.4).
    let constant = 1 /* byte 50 */
        + (4 + 14) /* "ssh-connection" */
        + (4 + 9) /* "publickey" */
        + 1 /* boolean TRUE */
        + (4 + 11) /* "ssh-ed25519" */
        + (4 + (4 + 11 + 4 + 32)); /* pubkey blob */
    let session = 4 + KMS_SSH_SESSION_ID_LEN;
    for username_len in 1..=KMS_SSH_USERNAME_MAX {
        let total = session + 4 + username_len + constant;
        assert_eq!(total, 143 + username_len);
        // The reconstructed input never fits a payload, so it is never sent.
        assert!(total > KMS_PAYLOAD_LEN);
    }
    assert_eq!(143 + KMS_SSH_USERNAME_MAX, 174);
    assert_eq!(143 + 1, 144);
    // What is actually sent fits with room to spare.
    const { assert!(SshSignRequestPayload::LEN <= KMS_PAYLOAD_LEN) };
    assert_eq!(
        SshSignRequestPayload::LEN,
        8 + KMS_SSH_PUBLIC_KEY_LEN + KMS_SSH_SESSION_ID_LEN + 1 + KMS_SSH_USERNAME_MAX
    );
    assert_eq!(SshSignResponsePayload::LEN, 8 + KMS_SSH_SIGNATURE_LEN + 32);

    // Section 4.3 vector: assemble the 147-byte input from its typed parts.
    let request = sign_request();
    let mut input = Vec::new();
    fn put_string(input: &mut Vec<u8>, value: &[u8]) {
        input.extend_from_slice(&(value.len() as u32).to_be_bytes());
        input.extend_from_slice(value);
    }
    put_string(&mut input, &request.session_id);
    input.push(50);
    put_string(&mut input, request.username());
    put_string(&mut input, b"ssh-connection");
    put_string(&mut input, b"publickey");
    input.push(1);
    put_string(&mut input, b"ssh-ed25519");
    let mut blob = Vec::new();
    put_string(&mut blob, b"ssh-ed25519");
    put_string(&mut blob, &request.public_key);
    assert_eq!(blob.len(), 51);
    put_string(&mut input, &blob);
    assert_eq!(input.len(), 147);
    assert_eq!(input, bytes_of(SIGN_INPUT_HEX));
}
