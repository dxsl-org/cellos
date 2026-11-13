//! KMS SSH host-trust store calls (opcodes 25-29; Phase 09 S9-3/S9-4c, served by KMS
//! since Phase 12).
//!
//! A query is staged like an SSH import: `BeginTrustQuery` declares the kind, port, field
//! lengths and the SHA-256 of `host || algorithm || key`, the bytes follow in canonical
//! 96-byte chunks, and `CommitTrustQuery` answers. Every reply is verified before a call
//! reports success. A caller without the outbound-connect right is answered
//! `ssh_trust::DENIED`, as the store has always answered it. Calls are bounded; a timeout
//! poisons the client like every other bounded KMS call.

use super::{KmsClient, KmsClientError, SSH_FRAME_TICKS};
use crate::session::sha256;
use abi::cell_session::ObjectRefV1;
use alloc::vec::Vec;
use kms_codec::{
    KmsErrorCode, KmsOpcode, KmsTrustQueryKind, SshImportCommitRequestPayload,
    SshTransferBeginResponsePayload, TrustChallengeCommitRequestPayload, TrustOutcomePayload,
    TrustQueryBeginRequestPayload, KMS_TRUST_ALGORITHM_MAX, KMS_TRUST_HOST_MAX, KMS_TRUST_KEY_MAX,
};
use wire::ipc::ssh_trust;

/// Bound of the two commits (27, 28): 3,000 ticks = 30 s. A commit reads the store (at most
/// 1 MiB) through the VFS, registers with or collects from the input owner, and an
/// accepted first trust publishes the store again.
pub const TRUST_COMMIT_TICKS: u64 = 3_000;

fn object_ref((id, generation): (u64, u64)) -> ObjectRefV1 {
    ObjectRefV1 { id, generation }
}

/// The store's refusal of a caller without the outbound-connect right.
fn denied_as_code<T>(
    result: Result<T, KmsClientError>,
    code: impl FnOnce(u8) -> T,
) -> Result<T, KmsClientError> {
    match result {
        Err(KmsClientError::Service(KmsErrorCode::PermissionDenied)) => Ok(code(ssh_trust::DENIED)),
        other => other,
    }
}

impl KmsClient {
    /// Host-trust lookup: one of `wire::ipc::ssh_trust` for the configured `host`/`port`
    /// and the complete SSH key blob `key`.
    pub fn trust_lookup(
        &self,
        host: &str,
        port: u16,
        algorithm: &str,
        key: &[u8],
    ) -> Result<u8, KmsClientError> {
        let outcome = self
            .trust_query(
                KmsTrustQueryKind::Lookup,
                host,
                port,
                algorithm,
                key,
                ObjectRefV1::ZERO,
                0,
            )
            .and_then(|o| match o.challenged {
                0 => Ok(o.outcome),
                _ => Err(KmsClientError::MismatchedResponse),
            });
        denied_as_code(outcome, |code| code)
    }

    /// First trust (S9-4c): ask for a challenge for a host and port the store does not
    /// hold. `Ok(Ok((confirmation, challenge)))` to begin and commit; `Ok(Err(code))` with
    /// an `wire::ipc::ssh_trust` refusal (`KNOWN_EXACT`/`MISMATCH` need no prompt).
    /// `session`/`epoch` are the caller's own terminal session the confirmation runs on.
    pub fn trust_challenge(
        &self,
        host: &str,
        port: u16,
        algorithm: &str,
        key: &[u8],
        session: ObjectRefV1,
        epoch: u64,
    ) -> Result<Result<(ObjectRefV1, ObjectRefV1), u8>, KmsClientError> {
        if session.is_zero() {
            return Err(KmsClientError::InvalidPayload);
        }
        let outcome = self
            .trust_query(
                KmsTrustQueryKind::Challenge,
                host,
                port,
                algorithm,
                key,
                session,
                epoch,
            )
            .map(|o| match o.challenged {
                0 => Err(o.outcome),
                _ => Ok((object_ref(o.confirmation), object_ref(o.challenge))),
            });
        denied_as_code(outcome, Err)
    }

    /// First trust (S9-4c): complete the caller's own challenge; one of
    /// `wire::ipc::ssh_trust` (`PENDING` until the confirmation is decided).
    pub fn trust_commit(&self, challenge: ObjectRefV1) -> Result<u8, KmsClientError> {
        if challenge.is_zero() {
            return Err(KmsClientError::InvalidPayload);
        }
        let payload = TrustChallengeCommitRequestPayload {
            challenge: (challenge.id, challenge.generation),
        }
        .encode();
        let outcome = self
            .call_ssh(
                KmsOpcode::CommitTrustChallenge,
                &payload,
                TRUST_COMMIT_TICKS,
            )
            .and_then(|response| self.decode_payload(&response, TrustOutcomePayload::decode))
            .and_then(|o| match o.challenged {
                0 => Ok(o.outcome),
                _ => Err(KmsClientError::MismatchedResponse),
            });
        denied_as_code(outcome, |code| code)
    }

    /// Begin, canonical chunks, commit. Aborts the query (best effort) on any failure.
    #[allow(clippy::too_many_arguments)]
    fn trust_query(
        &self,
        kind: KmsTrustQueryKind,
        host: &str,
        port: u16,
        algorithm: &str,
        key: &[u8],
        session: ObjectRefV1,
        epoch: u64,
    ) -> Result<TrustOutcomePayload, KmsClientError> {
        if port == 0
            || !(1..=KMS_TRUST_HOST_MAX).contains(&host.len())
            || !(1..=KMS_TRUST_ALGORITHM_MAX).contains(&algorithm.len())
            || !(1..=KMS_TRUST_KEY_MAX).contains(&key.len())
        {
            return Err(KmsClientError::InvalidPayload);
        }
        let mut staged = Vec::new();
        staged
            .try_reserve_exact(host.len() + algorithm.len() + key.len())
            .map_err(|_| KmsClientError::InvalidPayload)?;
        staged.extend_from_slice(host.as_bytes());
        staged.extend_from_slice(algorithm.as_bytes());
        staged.extend_from_slice(key);
        let digest = sha256(&staged);
        let payload = TrustQueryBeginRequestPayload {
            kind,
            algorithm_len: algorithm.len() as u8,
            port,
            host_len: host.len() as u16,
            key_len: key.len() as u16,
            epoch,
            session: (session.id, session.generation),
            query_sha256: digest,
        }
        .encode();
        let response = self.call_ssh(KmsOpcode::BeginTrustQuery, &payload, SSH_FRAME_TICKS)?;
        let begin = self.decode_payload(&response, SshTransferBeginResponsePayload::decode)?;
        if begin.total_len as usize != staged.len() {
            return Err(KmsClientError::MismatchedResponse);
        }
        let result = self
            .send_chunks(&begin, &staged, KmsOpcode::TrustQueryChunk)
            .and_then(|()| {
                let commit = SshImportCommitRequestPayload {
                    handle: begin.handle,
                    chunk_count: begin.chunk_count,
                    reserved: 0,
                    blob_sha256: digest,
                }
                .encode();
                self.call_ssh(KmsOpcode::CommitTrustQuery, &commit, TRUST_COMMIT_TICKS)
            })
            .and_then(|response| self.decode_payload(&response, TrustOutcomePayload::decode));
        if result.is_err() {
            let _ = self.abort(KmsOpcode::AbortTrustQuery, begin.handle);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::boxed::Box;
    use alloc::rc::Rc;
    use core::cell::RefCell;
    use kms_codec::{
        KmsRequestV1, KmsResponseV1, SshChunkAckPayload, SshChunkRequestPayload, KMS_MESSAGE_LEN,
    };

    type Frame = [u8; KMS_MESSAGE_LEN];

    fn reply(frame: &Frame, payload: Result<&[u8], KmsErrorCode>) -> Frame {
        let request = KmsRequestV1::from_bytes(frame).unwrap();
        let opcode = request.opcode().unwrap();
        match payload {
            Ok(body) => KmsResponseV1::ok(opcode, request.request_id, body).unwrap(),
            Err(code) => KmsResponseV1::error(opcode, request.request_id, code),
        }
        .to_bytes()
    }

    /// A service that stages faithfully and answers `outcome` at commit; it records the
    /// staged bytes and the opcodes it saw.
    fn service(
        outcome: Result<TrustOutcomePayload, KmsErrorCode>,
        seen: Rc<
            RefCell<(
                Vec<KmsOpcode>,
                Vec<u8>,
                Option<TrustQueryBeginRequestPayload>,
            )>,
        >,
    ) -> impl FnMut(&Frame) -> Option<Frame> {
        move |frame: &Frame| {
            let request = KmsRequestV1::from_bytes(frame).unwrap();
            let opcode = request.opcode().unwrap();
            let body = request.payload().unwrap();
            let mut seen = seen.borrow_mut();
            seen.0.push(opcode);
            Some(match opcode {
                KmsOpcode::BeginTrustQuery => {
                    let begin = TrustQueryBeginRequestPayload::decode(body).unwrap();
                    seen.2 = Some(begin);
                    seen.1.clear();
                    let total = begin.total_len();
                    let reply_body = SshTransferBeginResponsePayload {
                        handle: 0x51,
                        total_len: total,
                        chunk_count: total.div_ceil(96),
                        chunk_capacity: 96,
                        reserved: [0; 6],
                    }
                    .encode();
                    reply(frame, Ok(&reply_body))
                }
                KmsOpcode::TrustQueryChunk => {
                    let chunk = SshChunkRequestPayload::decode(body).unwrap();
                    seen.1.extend_from_slice(chunk.data());
                    let received = seen.1.len() as u32;
                    let ack = SshChunkAckPayload {
                        handle: 0x51,
                        next_chunk_index: chunk.chunk_index + 1,
                        received_len: received,
                    }
                    .encode();
                    reply(frame, Ok(&ack))
                }
                KmsOpcode::CommitTrustQuery | KmsOpcode::CommitTrustChallenge => match outcome {
                    Ok(o) => reply(frame, Ok(&o.encode())),
                    Err(code) => reply(frame, Err(code)),
                },
                KmsOpcode::AbortTrustQuery => reply(frame, Ok(&[])),
                _ => reply(frame, Err(KmsErrorCode::UnknownOpcode)),
            })
        }
    }

    type Seen = Rc<
        RefCell<(
            Vec<KmsOpcode>,
            Vec<u8>,
            Option<TrustQueryBeginRequestPayload>,
        )>,
    >;

    #[test]
    fn lookup_stages_host_algorithm_and_key_and_answers_the_code() {
        let seen: Seen = Rc::default();
        let kms = KmsClient::scripted(Box::new(service(
            Ok(TrustOutcomePayload::outcome(ssh_trust::MISMATCH)),
            seen.clone(),
        )));
        let key = [7u8; 200];
        assert_eq!(
            kms.trust_lookup("gateway", 2222, "ssh-ed25519", &key),
            Ok(ssh_trust::MISMATCH)
        );
        let (ops, staged, begin) = &*seen.borrow();
        let mut expected = Vec::from(&b"gatewayssh-ed25519"[..]);
        expected.extend_from_slice(&key);
        assert_eq!(staged, &expected);
        let begin = begin.unwrap();
        assert_eq!(begin.kind, KmsTrustQueryKind::Lookup);
        assert_eq!(
            (begin.port, begin.host_len, begin.algorithm_len),
            (2222, 7, 11)
        );
        assert_eq!(begin.query_sha256, sha256(&expected));
        assert_eq!(begin.session, (0, 0));
        assert_eq!(
            ops.as_slice(),
            [
                KmsOpcode::BeginTrustQuery,
                KmsOpcode::TrustQueryChunk,
                KmsOpcode::TrustQueryChunk,
                KmsOpcode::TrustQueryChunk,
                KmsOpcode::CommitTrustQuery
            ]
        );
    }

    #[test]
    fn challenge_answers_refs_or_a_refusal_and_denial_is_a_code() {
        let seen: Seen = Rc::default();
        let kms = KmsClient::scripted(Box::new(service(
            Ok(TrustOutcomePayload::challenged((500, 7), (1, 1))),
            seen.clone(),
        )));
        let session = ObjectRefV1 {
            id: 41,
            generation: 3,
        };
        assert_eq!(
            kms.trust_challenge("gateway", 22, "ssh-ed25519", b"k", session, 4),
            Ok(Ok((
                ObjectRefV1 {
                    id: 500,
                    generation: 7
                },
                ObjectRefV1 {
                    id: 1,
                    generation: 1
                }
            )))
        );
        let begin = seen.borrow().2.unwrap();
        assert_eq!(begin.kind, KmsTrustQueryKind::Challenge);
        assert_eq!((begin.session, begin.epoch), ((41, 3), 4));

        let kms = KmsClient::scripted(Box::new(service(
            Ok(TrustOutcomePayload::outcome(ssh_trust::KNOWN_EXACT)),
            Rc::default(),
        )));
        assert_eq!(
            kms.trust_challenge("gateway", 22, "ssh-ed25519", b"k", session, 4),
            Ok(Err(ssh_trust::KNOWN_EXACT))
        );
        let kms = KmsClient::scripted(Box::new(service(
            Err(KmsErrorCode::PermissionDenied),
            Rc::default(),
        )));
        assert_eq!(
            kms.trust_challenge("gateway", 22, "ssh-ed25519", b"k", session, 4),
            Ok(Err(ssh_trust::DENIED))
        );
        assert_eq!(
            kms.trust_lookup("gateway", 22, "ssh-ed25519", b"k"),
            Ok(ssh_trust::DENIED)
        );
        assert_eq!(
            kms.trust_commit(ObjectRefV1 {
                id: 1,
                generation: 1
            }),
            Ok(ssh_trust::DENIED)
        );
    }

    #[test]
    fn commit_polls_one_frame_and_a_failed_query_is_aborted() {
        let seen: Seen = Rc::default();
        let kms = KmsClient::scripted(Box::new(service(
            Ok(TrustOutcomePayload::outcome(ssh_trust::PENDING)),
            seen.clone(),
        )));
        let challenge = ObjectRefV1 {
            id: 1,
            generation: 1,
        };
        assert_eq!(kms.trust_commit(challenge), Ok(ssh_trust::PENDING));
        assert_eq!(seen.borrow().0, [KmsOpcode::CommitTrustChallenge]);
        let (frame, ticks) = kms.sent_frames()[0];
        assert_eq!(ticks, TRUST_COMMIT_TICKS);
        assert_eq!(
            &frame[16..32],
            &[1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0]
        );

        let seen: Seen = Rc::default();
        let kms = KmsClient::scripted(Box::new(service(
            Err(KmsErrorCode::TrustOrderInvalid),
            seen.clone(),
        )));
        assert_eq!(
            kms.trust_lookup("h", 22, "ssh-ed25519", b"k"),
            Err(KmsClientError::Service(KmsErrorCode::TrustOrderInvalid))
        );
        assert_eq!(seen.borrow().0.last(), Some(&KmsOpcode::AbortTrustQuery));
        // Bounds are checked before anything is sent.
        let kms = KmsClient::scripted(Box::new(|_: &Frame| None));
        for (host, port, key) in [("", 22, &b"k"[..]), ("h", 0, b"k"), ("h", 22, b"")] {
            assert_eq!(
                kms.trust_lookup(host, port, "ssh-ed25519", key),
                Err(KmsClientError::InvalidPayload)
            );
        }
        assert_eq!(
            kms.trust_commit(ObjectRefV1::ZERO),
            Err(KmsClientError::InvalidPayload)
        );
        assert!(kms.sent_frames().is_empty());
    }
}
