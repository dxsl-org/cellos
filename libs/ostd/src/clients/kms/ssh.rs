//! KMS SSH custody calls (K2 opcodes 15-24) for the one KMS-held ed25519 identity.
//!
//! `cellbox ssh` uses only the login calls: [`KmsClient::ssh_key_status`],
//! [`KmsClient::ssh_stage_secret`] (with its three parts) and
//! [`KmsClient::ssh_sign_userauth`]. The import and delete calls belong to the admin cell
//! (K7); KMS enforces the caller class, so a login Cell that calls them is answered
//! `PermissionDenied`.
//!
//! Every call here is bounded: the service sends no reply at all to a frame it cannot
//! decode, so the unbounded receive of the older calls would block forever. A timeout
//! poisons the client (see [`KmsClientError::Timeout`]).
//!
//! Replies are verified before a call reports success: handle, canonical chunk plan,
//! chunk index and cumulative length, and the echo of the payload `request_id`.

use super::{KmsClient, KmsClientError, Wiped};
use crate::session::{sha256, zeroize};
use types::kms::{
    KMS_SSH_CHUNK_LEN, KMS_SSH_IMPORT_MAX_LEN, KMS_SSH_KEY_NAME_MAX, KMS_SSH_PUBLIC_KEY_LEN,
    KMS_SSH_SECRET_MAX, KMS_SSH_SESSION_ID_LEN, KMS_SSH_USERNAME_MAX, KmsOpcode,
    SshAbortRequestPayload, SshChunkAckPayload, SshChunkRequestPayload, SshDeleteRequestPayload,
    SshImportBeginRequestPayload, SshImportCommitRequestPayload, SshKeyStatusPayload,
    SshSignRequestPayload, SshSignResponsePayload, SshStatusRequestPayload,
    SshTransferBeginResponsePayload, SshUnlockBeginRequestPayload, ssh_chunk_len,
    validate_ssh_key_name, validate_ssh_username,
};

/// Receive bound of the cheap SSH opcodes (15-20, 22-24): 500 ticks = 5 s. A placeholder
/// until K5/K8 measure the target (K6 CP1 O-K6-3).
pub const SSH_FRAME_TICKS: u64 = 500;
/// Receive bound of the sign opcode (21), which carries the bcrypt KDF of a wrapped key:
/// 6,000 ticks = 60 s, below the 120 s Auth budget of `cellbox ssh`. A placeholder until
/// K5/K8 measure the target (K6 CP1 O-K6-3).
pub const SSH_SIGN_TICKS: u64 = 6_000;

fn padded_name(name: &[u8]) -> Result<[u8; KMS_SSH_KEY_NAME_MAX], KmsClientError> {
    if !validate_ssh_key_name(name) {
        return Err(KmsClientError::InvalidPayload);
    }
    let mut field = [0u8; KMS_SSH_KEY_NAME_MAX];
    field[..name.len()].copy_from_slice(name);
    Ok(field)
}

impl KmsClient {
    /// Public facts of the stored identity (opcode 19). An empty `name` is the leaf
    /// query, which succeeds with `key_present == 0` when nothing is stored; a named
    /// query that does not name the stored key is a service error, so a named reply
    /// without a key is a protocol violation here.
    pub fn ssh_key_status(&self, name: &[u8]) -> Result<SshKeyStatusPayload, KmsClientError> {
        let mut field = [0u8; KMS_SSH_KEY_NAME_MAX];
        if !name.is_empty() {
            field = padded_name(name)?;
        }
        let payload = SshStatusRequestPayload {
            name_len: name.len() as u8,
            name: field,
        }
        .encode();
        let response = self.call_ssh(KmsOpcode::GetSshKeyStatus, &payload, SSH_FRAME_TICKS)?;
        let status = self.decode_payload(&response, SshKeyStatusPayload::decode)?;
        if !name.is_empty() && status.key_present != 1 {
            return Err(KmsClientError::MismatchedResponse);
        }
        Ok(status)
    }

    /// Open the one-shot unlock slot of the wrapped identity (opcode 22). `secret_len` is
    /// the passphrase length, `1..=1,024`; the service answers the canonical chunk plan.
    pub fn ssh_unlock_begin(
        &self,
        public_key: &[u8; KMS_SSH_PUBLIC_KEY_LEN],
        secret_len: usize,
    ) -> Result<SshTransferBeginResponsePayload, KmsClientError> {
        if secret_len == 0 || secret_len > KMS_SSH_SECRET_MAX {
            return Err(KmsClientError::InvalidPayload);
        }
        let request_id = self.next_payload_id()?;
        let payload = SshUnlockBeginRequestPayload {
            request_id,
            public_key: *public_key,
            secret_len: secret_len as u16,
            reserved0: 0,
            reserved1: 0,
        }
        .encode();
        let response = self.call_ssh(KmsOpcode::BeginSshUnlock, &payload, SSH_FRAME_TICKS)?;
        let begin = self.decode_payload(&response, SshTransferBeginResponsePayload::decode)?;
        if begin.total_len as usize != secret_len {
            return Err(KmsClientError::MismatchedResponse);
        }
        Ok(begin)
    }

    /// One ordered chunk (`1..=96` bytes) of the staged passphrase (opcode 23). The ack
    /// must name the same handle and the next index.
    pub fn ssh_unlock_chunk(
        &self,
        handle: u64,
        index: u32,
        data: &[u8],
    ) -> Result<SshChunkAckPayload, KmsClientError> {
        self.chunk(KmsOpcode::SshUnlockChunk, handle, index, data)
    }

    /// Destroy the unlock slot (opcode 24). Best effort for a caller that is giving up.
    pub fn ssh_unlock_abort(&self, handle: u64) -> Result<(), KmsClientError> {
        self.abort(KmsOpcode::AbortSshUnlock, handle)
    }

    /// Ask KMS to sign the userauth challenge for `username` over `session_id` with the
    /// stored identity named by `public_key` (opcode 21). KMS reconstructs the RFC 4252
    /// section 7 input itself; the reply echoes the payload id.
    pub fn ssh_sign_userauth(
        &self,
        public_key: &[u8; KMS_SSH_PUBLIC_KEY_LEN],
        session_id: &[u8; KMS_SSH_SESSION_ID_LEN],
        username: &[u8],
    ) -> Result<SshSignResponsePayload, KmsClientError> {
        if session_id.iter().all(|byte| *byte == 0) || !validate_ssh_username(username) {
            return Err(KmsClientError::InvalidPayload);
        }
        let mut field = [0u8; KMS_SSH_USERNAME_MAX];
        field[..username.len()].copy_from_slice(username);
        let request_id = self.next_payload_id()?;
        let payload = SshSignRequestPayload {
            request_id,
            public_key: *public_key,
            session_id: *session_id,
            username_len: username.len() as u8,
            username: field,
        }
        .encode();
        let response = self.call_ssh(KmsOpcode::SignSshUserauth, &payload, SSH_SIGN_TICKS)?;
        let signed = self.decode_payload(&response, SshSignResponsePayload::decode)?;
        if signed.request_id != request_id {
            return Err(KmsClientError::MismatchedResponse);
        }
        Ok(signed)
    }

    /// Stage `secret` (the passphrase of the wrapped identity, `1..=1,024` bytes) for the
    /// next [`Self::ssh_sign_userauth`]: Begin, then ceil(len / 96) ordered chunks. Every
    /// reply is verified (handle, index, cumulative length). On any failure the slot is
    /// aborted (best effort) and the error returned. The caller owns `secret` and wipes it.
    pub fn ssh_stage_secret(
        &self,
        public_key: &[u8; KMS_SSH_PUBLIC_KEY_LEN],
        secret: &[u8],
    ) -> Result<(), KmsClientError> {
        let begin = self.ssh_unlock_begin(public_key, secret.len())?;
        let result = self.send_chunks(&begin, secret, KmsOpcode::SshUnlockChunk);
        if result.is_err() {
            let _ = self.ssh_unlock_abort(begin.handle);
        }
        result
    }

    /// Admin only (K7): open an import of a `total_len`-byte OpenSSH blob whose SHA-256
    /// is `blob_sha256` (opcode 15).
    pub fn ssh_import_begin(
        &self,
        name: &[u8],
        total_len: u32,
        blob_sha256: &[u8; 32],
    ) -> Result<SshTransferBeginResponsePayload, KmsClientError> {
        if total_len == 0 || total_len as usize > KMS_SSH_IMPORT_MAX_LEN {
            return Err(KmsClientError::InvalidPayload);
        }
        let padded = padded_name(name)?;
        let request_id = self.next_payload_id()?;
        let payload = SshImportBeginRequestPayload {
            request_id,
            total_len,
            name_len: name.len() as u8,
            reserved: [0; 3],
            blob_sha256: *blob_sha256,
            name: padded,
        }
        .encode();
        let response = self.call_ssh(KmsOpcode::BeginSshKeyImport, &payload, SSH_FRAME_TICKS)?;
        let begin = self.decode_payload(&response, SshTransferBeginResponsePayload::decode)?;
        if begin.total_len != total_len {
            return Err(KmsClientError::MismatchedResponse);
        }
        Ok(begin)
    }

    /// Admin only (K7): one ordered import chunk (opcode 16).
    pub fn ssh_import_chunk(
        &self,
        handle: u64,
        index: u32,
        data: &[u8],
    ) -> Result<SshChunkAckPayload, KmsClientError> {
        self.chunk(KmsOpcode::ImportSshKeyChunk, handle, index, data)
    }

    /// Admin only (K7): commit the import after `chunk_count` chunks (opcode 17); the reply
    /// is the status of the stored key.
    pub fn ssh_import_commit(
        &self,
        handle: u64,
        chunk_count: u32,
        blob_sha256: &[u8; 32],
    ) -> Result<SshKeyStatusPayload, KmsClientError> {
        if handle == 0 {
            return Err(KmsClientError::InvalidPayload);
        }
        let payload = SshImportCommitRequestPayload {
            handle,
            chunk_count,
            reserved: 0,
            blob_sha256: *blob_sha256,
        }
        .encode();
        let response = self.call_ssh(KmsOpcode::CommitSshKeyImport, &payload, SSH_FRAME_TICKS)?;
        let status = self.decode_payload(&response, SshKeyStatusPayload::decode)?;
        if status.key_present != 1 {
            return Err(KmsClientError::MismatchedResponse);
        }
        Ok(status)
    }

    /// Admin only (K7): destroy the pending import (opcode 18).
    pub fn ssh_import_abort(&self, handle: u64) -> Result<(), KmsClientError> {
        self.abort(KmsOpcode::AbortSshKeyImport, handle)
    }

    /// Admin only (K7): delete the stored identity `name` whose public key is `public_key`
    /// (opcode 20).
    pub fn ssh_delete(
        &self,
        name: &[u8],
        public_key: &[u8; KMS_SSH_PUBLIC_KEY_LEN],
    ) -> Result<(), KmsClientError> {
        let padded = padded_name(name)?;
        let request_id = self.next_payload_id()?;
        let payload = SshDeleteRequestPayload {
            request_id,
            public_key: *public_key,
            name_len: name.len() as u8,
            name: padded,
        }
        .encode();
        let response = self.call_ssh(KmsOpcode::DeleteSshKey, &payload, SSH_FRAME_TICKS)?;
        self.expect_empty(&response)
    }

    /// Admin only (K7): import `blob` (a binary OpenSSH private key, at most 64 KiB) as
    /// `name`: Begin, canonical chunks, Commit, with every reply verified. Aborts the
    /// import (best effort) on any failure before the commit.
    pub fn ssh_import(
        &self,
        name: &[u8],
        blob: &[u8],
    ) -> Result<SshKeyStatusPayload, KmsClientError> {
        if blob.is_empty() || blob.len() > KMS_SSH_IMPORT_MAX_LEN {
            return Err(KmsClientError::InvalidPayload);
        }
        let digest = sha256(blob);
        let begin = self.ssh_import_begin(name, blob.len() as u32, &digest)?;
        let staged = self.send_chunks(&begin, blob, KmsOpcode::ImportSshKeyChunk);
        if staged.is_err() {
            let _ = self.ssh_import_abort(begin.handle);
            staged?;
        }
        let committed = self.ssh_import_commit(begin.handle, begin.chunk_count, &digest);
        if committed.is_err() {
            let _ = self.ssh_import_abort(begin.handle);
        }
        committed
    }

    /// Every canonical chunk of `data` under `begin`, each ack verified.
    fn send_chunks(
        &self,
        begin: &SshTransferBeginResponsePayload,
        data: &[u8],
        opcode: KmsOpcode,
    ) -> Result<(), KmsClientError> {
        let total = u32::try_from(data.len()).map_err(|_| KmsClientError::InvalidPayload)?;
        let mut offset = 0usize;
        for index in 0..begin.chunk_count {
            let take =
                ssh_chunk_len(total, index).ok_or(KmsClientError::MismatchedResponse)? as usize;
            let ack = self.chunk(opcode, begin.handle, index, &data[offset..offset + take])?;
            offset += take;
            if ack.received_len as usize != offset {
                return Err(KmsClientError::MismatchedResponse);
            }
        }
        if offset != data.len() {
            return Err(KmsClientError::MismatchedResponse);
        }
        Ok(())
    }

    /// One transfer chunk (ops 16 and 23). Chunk bytes may be a passphrase, so every
    /// local copy of them is wiped before this returns.
    fn chunk(
        &self,
        opcode: KmsOpcode,
        handle: u64,
        index: u32,
        data: &[u8],
    ) -> Result<SshChunkAckPayload, KmsClientError> {
        if handle == 0 || data.is_empty() || data.len() > KMS_SSH_CHUNK_LEN {
            return Err(KmsClientError::InvalidPayload);
        }
        let mut chunk = SshChunkRequestPayload {
            handle,
            chunk_index: index,
            chunk_len: data.len() as u16,
            reserved: 0,
            chunk: [0; KMS_SSH_CHUNK_LEN],
        };
        chunk.chunk[..data.len()].copy_from_slice(data);
        let payload = Wiped(chunk.encode());
        zeroize(&mut chunk.chunk);
        let response = self.call_ssh(opcode, &payload.0, SSH_FRAME_TICKS)?;
        drop(payload);
        let ack = self.decode_payload(&response, SshChunkAckPayload::decode)?;
        if ack.handle != handle || ack.next_chunk_index != index.wrapping_add(1) {
            return Err(KmsClientError::MismatchedResponse);
        }
        Ok(ack)
    }

    /// Abort of an import (18) or an unlock (24): empty success.
    fn abort(&self, opcode: KmsOpcode, handle: u64) -> Result<(), KmsClientError> {
        if handle == 0 {
            return Err(KmsClientError::InvalidPayload);
        }
        let payload = SshAbortRequestPayload { handle }.encode();
        let response = self.call_ssh(opcode, &payload, SSH_FRAME_TICKS)?;
        self.expect_empty(&response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::boxed::Box;
    use alloc::rc::Rc;
    use alloc::vec::Vec;
    use core::cell::{Cell, RefCell};
    use types::kms::{
        KMS_MESSAGE_LEN, KmsCapabilityReadiness, KmsCustodyPosture, KmsErrorCode, KmsKeyAlgorithm,
        KmsProviderKind, KmsRequestV1, KmsResponseV1, KmsSshKeyCustody, KmsWireError,
    };

    type Frame = [u8; KMS_MESSAGE_LEN];

    // Public key and signing vector provenance: K2 CP1 section 4.3 / `k2-cp2-dossier.md`
    // (public key of `ssh-key-0.7.0-rc.11/tests/examples/id_ed25519.pub`, session id
    // 00..1f, username `user`); the layouts below are written byte by byte here, not
    // produced by the `types` encoders, so a layout drift in either place fails.
    const PUBLIC: [u8; 32] = [
        0xb3, 0x3e, 0xae, 0xf3, 0x7e, 0xa2, 0xdf, 0x7c, 0xaa, 0x01, 0x0d, 0xef, 0xde, 0xa3, 0x4e,
        0x24, 0x1f, 0x65, 0xf1, 0xb5, 0x29, 0xa4, 0xf4, 0x3e, 0xd1, 0x43, 0x27, 0xf5, 0xc5, 0x4a,
        0xab, 0x62,
    ];

    fn session_id() -> [u8; 32] {
        let mut id = [0u8; 32];
        for (i, byte) in id.iter_mut().enumerate() {
            *byte = i as u8;
        }
        id
    }

    fn frame_id(frame: &Frame) -> u32 {
        u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]])
    }

    fn opcode_of(frame: &Frame) -> KmsOpcode {
        KmsOpcode::try_from(frame[1]).unwrap()
    }

    fn payload_of(frame: &Frame) -> &[u8] {
        let len = u16::from_le_bytes([frame[8], frame[9]]) as usize;
        &frame[16..16 + len]
    }

    fn payload_id(frame: &Frame) -> u64 {
        u64::from_le_bytes(payload_of(frame)[..8].try_into().unwrap())
    }

    fn ok(frame: &Frame, payload: &[u8]) -> Frame {
        KmsResponseV1::ok(opcode_of(frame), frame_id(frame), payload)
            .unwrap()
            .to_bytes()
    }

    fn err(frame: &Frame, code: KmsErrorCode) -> Frame {
        KmsResponseV1::error(opcode_of(frame), frame_id(frame), code).to_bytes()
    }

    fn status(custody: KmsSshKeyCustody, present: bool) -> SshKeyStatusPayload {
        SshKeyStatusPayload {
            algorithm: KmsKeyAlgorithm::SshEd25519,
            readiness: KmsCapabilityReadiness::Ready,
            provider: KmsProviderKind::TestHooks,
            posture: KmsCustodyPosture::DevPostureVolatile,
            key_present: u8::from(present),
            custody: if present {
                custody
            } else {
                KmsSshKeyCustody::None
            },
            key_count: u16::from(present),
            store_revision: 7,
            public_key: if present { PUBLIC } else { [0; 32] },
            fingerprint_sha256: if present { [9; 32] } else { [0; 32] },
        }
    }

    fn begin_reply(handle: u64, total: u32) -> Vec<u8> {
        SshTransferBeginResponsePayload {
            handle,
            total_len: total,
            chunk_count: total.div_ceil(96),
            chunk_capacity: 96,
            reserved: [0; 6],
        }
        .encode()
        .to_vec()
    }

    fn ack_reply(handle: u64, received: u32) -> Vec<u8> {
        SshChunkAckPayload {
            handle,
            next_chunk_index: received.div_ceil(96),
            received_len: received,
        }
        .encode()
        .to_vec()
    }

    fn client(responder: impl FnMut(&Frame) -> Option<Frame> + 'static) -> KmsClient {
        KmsClient::scripted(Box::new(responder))
    }

    /// A service that follows the unlock protocol faithfully.
    fn unlock_service() -> impl FnMut(&Frame) -> Option<Frame> {
        let state = RefCell::new((0u32, 0u32));
        move |frame: &Frame| {
            let mut state = state.borrow_mut();
            let body = payload_of(frame);
            Some(match opcode_of(frame) {
                KmsOpcode::BeginSshUnlock => {
                    let len = u16::from_le_bytes([body[40], body[41]]) as u32;
                    *state = (len, 0);
                    ok(frame, &begin_reply(0x77, len))
                }
                KmsOpcode::SshUnlockChunk => {
                    state.1 += u16::from_le_bytes([body[12], body[13]]) as u32;
                    ok(frame, &ack_reply(0x77, state.1))
                }
                KmsOpcode::AbortSshUnlock => ok(frame, &[]),
                _ => err(frame, KmsErrorCode::UnknownOpcode),
            })
        }
    }

    #[test]
    fn status_request_frames_are_golden_for_leaf_and_named_queries() {
        let seen: Rc<RefCell<Vec<Frame>>> = Rc::default();
        let log = seen.clone();
        let reply = status(KmsSshKeyCustody::Plain, true);
        let leaf = status(KmsSshKeyCustody::Plain, false);
        let kms = client(move |frame| {
            log.borrow_mut().push(*frame);
            let body = payload_of(frame);
            Some(ok(
                frame,
                &if body[0] == 0 {
                    leaf.encode()
                } else {
                    reply.encode()
                },
            ))
        });
        assert_eq!(kms.ssh_key_status(b"").unwrap().key_present, 0);
        let named = kms.ssh_key_status(b"work-1").unwrap();
        assert_eq!(
            (named.custody, named.public_key),
            (KmsSshKeyCustody::Plain, PUBLIC)
        );
        let frames = seen.borrow();
        // Header: version 1, opcode 19, flags 0, frame id, payload_len 65, reserved 0.
        let mut header = [0u8; 16];
        header[0] = 1;
        header[1] = 19;
        header[4] = 1;
        header[8] = 65;
        assert_eq!(&frames[0][..16], &header);
        assert!(
            frames[0][16..].iter().all(|b| *b == 0),
            "leaf query: empty name, zero field"
        );
        header[4] = 2;
        assert_eq!(&frames[1][..16], &header);
        let mut expected = [0u8; 65];
        expected[0] = 6;
        expected[1..7].copy_from_slice(b"work-1");
        assert_eq!(payload_of(&frames[1]), &expected);
        assert!(
            frames[1][16 + 65..].iter().all(|b| *b == 0),
            "canonical zero tail"
        );
        // Local validation: nothing is sent for a bad name.
        drop(frames);
        for bad in [&b"bad name"[..], b"a.b", &[b'a'; 65]] {
            assert_eq!(kms.ssh_key_status(bad), Err(KmsClientError::InvalidPayload));
        }
        assert_eq!(kms.sent_frames().len(), 2);
    }

    #[test]
    fn a_named_status_reply_without_a_key_is_a_protocol_violation() {
        let leaf = status(KmsSshKeyCustody::Plain, false);
        let kms = client(move |frame| Some(ok(frame, &leaf.encode())));
        assert_eq!(
            kms.ssh_key_status(b"k"),
            Err(KmsClientError::MismatchedResponse)
        );
    }

    #[test]
    fn sign_request_is_golden_uses_the_long_bound_and_echoes_the_payload_id() {
        let kms = client(|frame| {
            let id = payload_id(frame);
            Some(ok(
                frame,
                &SshSignResponsePayload {
                    request_id: id,
                    signature: [0x5a; 64],
                    message_sha256: [3; 32],
                }
                .encode(),
            ))
        });
        let signed = kms
            .ssh_sign_userauth(&PUBLIC, &session_id(), b"user")
            .unwrap();
        assert_eq!(
            (signed.request_id, signed.signature, signed.message_sha256),
            (1, [0x5a; 64], [3; 32])
        );
        let sent = kms.sent_frames();
        let (frame, ticks) = &sent[0];
        assert_eq!(*ticks, SSH_SIGN_TICKS);
        assert_eq!(
            &frame[..16],
            &[1, 21, 0, 0, 1, 0, 0, 0, 104, 0, 0, 0, 0, 0, 0, 0]
        );
        let mut expected = Vec::new();
        expected.extend_from_slice(&1u64.to_le_bytes());
        expected.extend_from_slice(&PUBLIC);
        expected.extend_from_slice(&session_id());
        expected.push(4);
        expected.extend_from_slice(b"user");
        expected.resize(104, 0);
        assert_eq!(payload_of(frame), &expected[..]);
        assert!(frame[16 + 104..].iter().all(|b| *b == 0));
    }

    #[test]
    fn sign_refuses_bad_local_inputs_without_sending() {
        let kms = client(|frame| Some(err(frame, KmsErrorCode::InvalidRequest)));
        let zero = [0u8; 32];
        assert_eq!(
            kms.ssh_sign_userauth(&PUBLIC, &zero, b"user"),
            Err(KmsClientError::InvalidPayload)
        );
        for bad in [&b""[..], b"us er", b"a@b", &[b'u'; 32], b"\xff"] {
            assert_eq!(
                kms.ssh_sign_userauth(&PUBLIC, &session_id(), bad),
                Err(KmsClientError::InvalidPayload)
            );
        }
        assert!(kms.sent_frames().is_empty());
        // Edge usernames of the frozen charset are accepted.
        let _ = kms.ssh_sign_userauth(&PUBLIC, &session_id(), b"a.b_c-9");
        let _ = kms.ssh_sign_userauth(&PUBLIC, &session_id(), &[b'u'; 31]);
        assert_eq!(kms.sent_frames().len(), 2);
    }

    #[test]
    fn a_sign_reply_for_another_payload_id_is_rejected() {
        let kms = client(|frame| {
            let wrong = payload_id(frame) + 1;
            Some(ok(
                frame,
                &SshSignResponsePayload {
                    request_id: wrong,
                    signature: [1; 64],
                    message_sha256: [2; 32],
                }
                .encode(),
            ))
        });
        assert_eq!(
            kms.ssh_sign_userauth(&PUBLIC, &session_id(), b"user"),
            Err(KmsClientError::MismatchedResponse)
        );
    }

    #[test]
    fn staging_chunks_canonically_for_each_boundary_length() {
        // (secret length, chunk lengths): 1, 96, 97 and the 1,024-byte maximum.
        let cases: [(usize, &[usize]); 4] = [
            (1, &[1]),
            (96, &[96]),
            (97, &[96, 1]),
            (1024, &[96, 96, 96, 96, 96, 96, 96, 96, 96, 96, 64]),
        ];
        for (len, chunks) in cases {
            let kms = client(unlock_service());
            let secret: Vec<u8> = (0..len).map(|i| (i % 251) as u8 + 1).collect();
            kms.ssh_stage_secret(&PUBLIC, &secret).unwrap();
            let sent = kms.sent_frames();
            assert_eq!(
                sent.len(),
                1 + chunks.len(),
                "Begin plus one frame per chunk, len {len}"
            );
            // Begin: opcode 22, payload id 1, the public key, secret_len.
            let begin = &sent[0].0;
            assert_eq!(opcode_of(begin), KmsOpcode::BeginSshUnlock);
            assert_eq!(payload_of(begin).len(), 48);
            assert_eq!(payload_id(begin), 1);
            assert_eq!(&payload_of(begin)[8..40], &PUBLIC);
            assert_eq!(
                u16::from_le_bytes([payload_of(begin)[40], payload_of(begin)[41]]) as usize,
                len
            );
            let mut rebuilt = Vec::new();
            for (index, (frame, ticks)) in sent[1..].iter().enumerate() {
                assert_eq!(
                    (opcode_of(frame), *ticks),
                    (KmsOpcode::SshUnlockChunk, SSH_FRAME_TICKS)
                );
                let body = payload_of(frame);
                assert_eq!(body.len(), 112);
                assert_eq!(u64::from_le_bytes(body[..8].try_into().unwrap()), 0x77);
                assert_eq!(
                    u32::from_le_bytes(body[8..12].try_into().unwrap()),
                    index as u32
                );
                let n = u16::from_le_bytes([body[12], body[13]]) as usize;
                assert_eq!(n, chunks[index]);
                assert_eq!(&body[14..16], &[0, 0]);
                assert!(
                    body[16 + n..].iter().all(|b| *b == 0),
                    "zero after chunk_len"
                );
                rebuilt.extend_from_slice(&body[16..16 + n]);
            }
            assert_eq!(rebuilt, secret);
        }
    }

    #[test]
    fn staging_refuses_empty_and_oversized_secrets_locally() {
        let kms = client(unlock_service());
        assert_eq!(
            kms.ssh_stage_secret(&PUBLIC, b""),
            Err(KmsClientError::InvalidPayload)
        );
        assert_eq!(
            kms.ssh_stage_secret(&PUBLIC, &[b'x'; 1025]),
            Err(KmsClientError::InvalidPayload)
        );
        assert_eq!(
            kms.ssh_unlock_chunk(0, 0, b"x"),
            Err(KmsClientError::InvalidPayload)
        );
        assert_eq!(
            kms.ssh_unlock_chunk(1, 0, b""),
            Err(KmsClientError::InvalidPayload)
        );
        assert_eq!(
            kms.ssh_unlock_chunk(1, 0, &[1u8; 97]),
            Err(KmsClientError::InvalidPayload)
        );
        assert!(kms.sent_frames().is_empty());
    }

    #[test]
    fn unlock_then_sign_uses_begin_chunks_sign_with_increasing_nonzero_ids() {
        let unlock = RefCell::new(unlock_service());
        let kms = client(move |frame| match opcode_of(frame) {
            KmsOpcode::SignSshUserauth => Some(ok(
                frame,
                &SshSignResponsePayload {
                    request_id: payload_id(frame),
                    signature: [7; 64],
                    message_sha256: [8; 32],
                }
                .encode(),
            )),
            _ => (*unlock.borrow_mut())(frame),
        });
        kms.ssh_stage_secret(&PUBLIC, b"correct horse").unwrap();
        kms.ssh_sign_userauth(&PUBLIC, &session_id(), b"tester")
            .unwrap();
        kms.ssh_sign_userauth(&PUBLIC, &session_id(), b"tester")
            .unwrap();
        let sent = kms.sent_frames();
        let order: Vec<KmsOpcode> = sent.iter().map(|(f, _)| opcode_of(f)).collect();
        assert_eq!(
            order,
            [
                KmsOpcode::BeginSshUnlock,
                KmsOpcode::SshUnlockChunk,
                KmsOpcode::SignSshUserauth,
                KmsOpcode::SignSshUserauth
            ]
        );
        // Payload ids exist on Begin and Sign only: 1, 2, 3, strictly increasing, nonzero.
        let ids = [
            payload_id(&sent[0].0),
            payload_id(&sent[2].0),
            payload_id(&sent[3].0),
        ];
        assert_eq!(ids, [1, 2, 3]);
        // Frame ids are distinct and nonzero as well.
        let frames: Vec<u32> = sent.iter().map(|(f, _)| frame_id(f)).collect();
        assert_eq!(frames, [1, 2, 3, 4]);
    }

    #[test]
    fn a_deviating_chunk_ack_fails_the_stage_and_aborts_the_slot() {
        // Each deviation of the ack of chunk 0 (secret of 100 bytes: chunk 0 is 96).
        let skews = [
            "another handle",
            "short cumulative length",
            "long cumulative length",
        ];
        for (variant, what) in skews.into_iter().enumerate() {
            let kms = client(move |frame| {
                Some(match opcode_of(frame) {
                    KmsOpcode::BeginSshUnlock => ok(frame, &begin_reply(0x77, 100)),
                    KmsOpcode::SshUnlockChunk => ok(
                        frame,
                        &match variant {
                            0 => ack_reply(0x78, 96),
                            1 => ack_reply(0x77, 95),
                            _ => ack_reply(0x77, 97),
                        },
                    ),
                    _ => ok(frame, &[]),
                })
            });
            assert_eq!(
                kms.ssh_stage_secret(&PUBLIC, &[b'p'; 100]),
                Err(KmsClientError::MismatchedResponse),
                "{what}"
            );
            let sent = kms.sent_frames();
            let last = &sent.last().unwrap().0;
            assert_eq!(opcode_of(last), KmsOpcode::AbortSshUnlock, "{what}");
            assert_eq!(payload_of(last), &0x77u64.to_le_bytes(), "{what}");
            assert_eq!(sent.len(), 3, "{what}: begin, one chunk, abort");
        }
        // A Begin that names a different length is refused before any chunk.
        let kms = client(|frame| Some(ok(frame, &begin_reply(0x77, 50))));
        assert_eq!(
            kms.ssh_stage_secret(&PUBLIC, &[b'p'; 100]),
            Err(KmsClientError::MismatchedResponse)
        );
        assert_eq!(kms.sent_frames().len(), 1);
    }

    #[test]
    fn reply_envelope_deviations_are_rejected() {
        let wrong_opcode = client(|frame| {
            Some(
                KmsResponseV1::ok(KmsOpcode::GetSshKeyStatus, frame_id(frame), &[])
                    .unwrap()
                    .to_bytes(),
            )
        });
        assert_eq!(
            wrong_opcode.ssh_unlock_abort(1),
            Err(KmsClientError::MismatchedResponse)
        );
        let wrong_id = client(|frame| {
            Some(
                KmsResponseV1::ok(opcode_of(frame), frame_id(frame) + 1, &[])
                    .unwrap()
                    .to_bytes(),
            )
        });
        assert_eq!(
            wrong_id.ssh_unlock_abort(1),
            Err(KmsClientError::MismatchedResponse)
        );
        let torn = client(|frame| {
            let mut reply = ok(frame, &[]);
            reply[0] = 2; // unsupported version
            Some(reply)
        });
        assert!(matches!(
            torn.ssh_unlock_abort(1),
            Err(KmsClientError::Wire(KmsWireError::UnsupportedVersion(2)))
        ));
        // A success whose payload is not the empty one an abort answers.
        let fat = client(|frame| Some(ok(frame, &[1])));
        assert_eq!(fat.ssh_unlock_abort(1), Err(KmsClientError::InvalidPayload));
        // A status payload that violates the presence invariants.
        let mut bad = status(KmsSshKeyCustody::Plain, true).encode();
        bad[5] = 0;
        let kms = client(move |frame| Some(ok(frame, &bad)));
        assert_eq!(kms.ssh_key_status(b""), Err(KmsClientError::InvalidPayload));
    }

    #[test]
    fn silence_is_a_timeout_and_poisons_the_client() {
        let calls = Rc::new(Cell::new(0u32));
        let counted = calls.clone();
        let kms = client(move |_| {
            counted.set(counted.get() + 1);
            None
        });
        assert_eq!(kms.ssh_key_status(b""), Err(KmsClientError::Timeout));
        assert_eq!(calls.get(), 1);
        // Nothing more is sent, whatever is asked, and the error stays the same.
        assert_eq!(kms.ssh_key_status(b""), Err(KmsClientError::Timeout));
        assert_eq!(
            kms.ssh_sign_userauth(&PUBLIC, &session_id(), b"user"),
            Err(KmsClientError::Timeout)
        );
        assert_eq!(
            kms.ssh_stage_secret(&PUBLIC, b"pw"),
            Err(KmsClientError::Timeout)
        );
        assert_eq!(kms.ssh_unlock_abort(1), Err(KmsClientError::Timeout));
        assert_eq!(calls.get(), 1);
        assert_eq!(kms.sent_frames().len(), 1);
    }

    #[test]
    fn a_timeout_in_the_middle_of_staging_stops_without_another_send() {
        let n = Rc::new(Cell::new(0u32));
        let counted = n.clone();
        let mut service = unlock_service();
        let kms = client(move |frame| {
            counted.set(counted.get() + 1);
            if counted.get() == 2 {
                None
            } else {
                service(frame)
            }
        });
        assert_eq!(
            kms.ssh_stage_secret(&PUBLIC, &[b'p'; 200]),
            Err(KmsClientError::Timeout)
        );
        // Begin and the silent first chunk; the abort never leaves a poisoned client.
        assert_eq!(kms.sent_frames().len(), 2);
    }

    #[test]
    fn service_errors_pass_through_one_to_one() {
        use KmsErrorCode as E;
        for code in [
            E::CallerUnattested,
            E::PermissionDenied,
            E::SshKeyUnavailable,
            E::SshKeyNotFound,
            E::SshHandleInvalid,
            E::SshOrderInvalid,
            E::SshUnlockRequired,
            E::SshUnlockFailed,
            E::SshUnlockLocked,
            E::InvalidSignature,
            E::InvalidRequest,
            E::SshStoreFull,
        ] {
            let kms = client(move |frame| Some(err(frame, code)));
            assert_eq!(kms.ssh_key_status(b"k"), Err(KmsClientError::Service(code)));
            assert_eq!(
                kms.ssh_sign_userauth(&PUBLIC, &session_id(), b"user"),
                Err(KmsClientError::Service(code))
            );
            assert_eq!(
                kms.ssh_stage_secret(&PUBLIC, b"pw"),
                Err(KmsClientError::Service(code))
            );
        }
        // An error code this client does not know is a wire error, never a service code.
        let kms = client(|frame| {
            let mut reply = err(frame, KmsErrorCode::SshUnlockLocked);
            reply[8] = 0x7f; // code 0x7f is not defined
            Some(reply)
        });
        assert_eq!(
            kms.ssh_key_status(b"k"),
            Err(KmsClientError::Wire(KmsWireError::UnknownErrorCode(0x7f)))
        );
    }

    #[test]
    fn the_request_and_frame_buffers_are_wiped_after_the_send() {
        // The chunk frame is the only one that carries secret bytes. The sent copy is
        // inspected by the script; the request the call was given is zero afterwards.
        let mut request = KmsRequestV1::new(KmsOpcode::SshUnlockChunk, 9, &[0xAA; 112]).unwrap();
        let kms = client(|frame| Some(ok(frame, &ack_reply(0x77, 1))));
        kms.call_bounded(&mut request, SSH_FRAME_TICKS).unwrap();
        assert!(request.payload.iter().all(|b| *b == 0));
        assert!(
            payload_of(&kms.sent_frames()[0].0)
                .iter()
                .all(|b| *b == 0xAA)
        );
        // `Wiped` zeroes its array on drop.
        let mut wiped = core::mem::ManuallyDrop::new(Wiped([0x5Au8; 128]));
        // SAFETY: dropped exactly once and not used as a `Wiped` afterwards; only its
        // bytes are read, which stay valid for the lifetime of the `ManuallyDrop`.
        unsafe { core::mem::ManuallyDrop::drop(&mut wiped) };
        assert!(wiped.0.iter().all(|b| *b == 0));
    }

    #[test]
    fn import_drives_begin_canonical_chunks_and_commit_with_the_blob_digest() {
        let blob: Vec<u8> = (0..250).map(|i| i as u8).collect();
        let digest = sha256(&blob);
        let got = Rc::new(RefCell::new(Vec::<u8>::new()));
        let sink = got.clone();
        let received = Cell::new(0u32);
        let kms = client(move |frame| {
            let body = payload_of(frame);
            Some(match opcode_of(frame) {
                KmsOpcode::BeginSshKeyImport => {
                    assert_eq!(&body[16..48], &digest, "digest of the blob");
                    assert_eq!(u32::from_le_bytes(body[8..12].try_into().unwrap()), 250);
                    assert_eq!((body[12], &body[48..54]), (6, &b"worker"[..]));
                    ok(frame, &begin_reply(0x31, 250))
                }
                KmsOpcode::ImportSshKeyChunk => {
                    let n = u16::from_le_bytes([body[12], body[13]]) as u32;
                    sink.borrow_mut()
                        .extend_from_slice(&body[16..16 + n as usize]);
                    received.set(received.get() + n);
                    ok(frame, &ack_reply(0x31, received.get()))
                }
                KmsOpcode::CommitSshKeyImport => {
                    assert_eq!(
                        u32::from_le_bytes(body[8..12].try_into().unwrap()),
                        3,
                        "chunk_count"
                    );
                    assert_eq!(&body[16..48], &digest);
                    ok(frame, &status(KmsSshKeyCustody::Plain, true).encode())
                }
                _ => err(frame, KmsErrorCode::UnknownOpcode),
            })
        });
        let stored = kms.ssh_import(b"worker", &blob).unwrap();
        assert_eq!((stored.key_present, stored.public_key), (1, PUBLIC));
        assert_eq!(*got.borrow(), blob);
        // 250 bytes = 96 + 96 + 58: Begin, 3 chunks, Commit; ids on Begin only here.
        assert_eq!(kms.sent_frames().len(), 5);
        assert_eq!(
            kms.ssh_import(b"worker", b""),
            Err(KmsClientError::InvalidPayload)
        );
        assert_eq!(
            kms.ssh_import(b"bad name", &blob),
            Err(KmsClientError::InvalidPayload)
        );
        assert_eq!(
            kms.ssh_import(b"w", &alloc::vec![0u8; KMS_SSH_IMPORT_MAX_LEN + 1]),
            Err(KmsClientError::InvalidPayload)
        );
    }

    #[test]
    fn delete_carries_name_key_and_a_fresh_payload_id() {
        let kms = client(|frame| Some(ok(frame, &[])));
        kms.ssh_delete(b"old", &PUBLIC).unwrap();
        let sent = kms.sent_frames();
        let body = payload_of(&sent[0].0);
        assert_eq!(
            (opcode_of(&sent[0].0), body.len(), payload_id(&sent[0].0)),
            (KmsOpcode::DeleteSshKey, 105, 1)
        );
        assert_eq!(&body[8..40], &PUBLIC);
        assert_eq!((body[40], &body[41..44]), (3, &b"old"[..]));
    }
}
