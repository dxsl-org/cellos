//! Unlock slot, sign, failure limiter, self-verification (rows 22-30).

use std::vec;
use std::vec::Vec;

use super::fixtures::*;
use super::*;
use crate::ssh::sign::FLIP_SIGNATURE;
use crate::ssh::unlock::UnlockSlot;
use crate::ssh::unwrap::KDF_CALLS;

fn kdf_calls() -> usize {
    KDF_CALLS.with(|calls| calls.get())
}

/// A service holding a crafted wrapped key (cheap KDF) under the name `wk`.
fn wrapped_service(pass: &[u8], rounds: u32) -> (KmsService, Actor, Actor, [u8; 32]) {
    let mut service = service();
    let admin = Actor::admin();
    let login = Actor::login();
    let blob = Craft::wrapped(&RFC8032_SEED_1, pass, &[0x11; 16], rounds).bytes();
    import_blob(&mut service, &admin, b"wk", &blob).unwrap();
    (service, admin, login, public_of(&RFC8032_SEED_1))
}

fn plain_service() -> (KmsService, Actor, Actor, [u8; 32]) {
    let mut service = service();
    let admin = Actor::admin();
    import_blob(&mut service, &admin, b"pk", &id_ed25519()).unwrap();
    (service, admin, Actor::login(), id_ed25519_public())
}

/// Whole-frame access for byte-identity assertions (service only).
pub(super) trait FrameAccess {
    fn frame(&mut self, opcode: KmsOpcode, payload: &[u8], actor: &Actor) -> [u8; 128];
}

impl FrameAccess for KmsService {
    fn frame(&mut self, opcode: KmsOpcode, payload: &[u8], actor: &Actor) -> [u8; 128] {
        let request = KmsRequestV1::new(opcode, 41, payload).unwrap().to_bytes();
        self.handle(
            &request,
            actor.tid,
            actor.identity(),
            registry(None, Some(8)),
        )
        .unwrap()
        .to_bytes()
    }
}

#[test]
fn plain_key_signs_without_unlock_and_leaves_no_slot() {
    let (mut service, _admin, login, public) = plain_service();
    let signed = sign(&mut service, &login, public, SESSION_ID, b"user").unwrap();
    assert_eq!(signed.request_id, 1);
    assert_eq!(signed.signature.len(), 64);
    assert!(matches!(service.ssh.unlock, UnlockSlot::Empty));
    assert_eq!(kdf_calls(), 0, "no KDF for a plain key");
    // Deterministic: the same input signs to the same bytes.
    let again = sign(&mut service, &login, public, SESSION_ID, b"user").unwrap();
    assert_eq!(again.signature, signed.signature);
    assert_eq!(again.message_sha256, signed.message_sha256);
}

#[test]
fn sign_unknown_public_key() {
    let (mut service, _admin, login, _public) = plain_service();
    let before = snapshot(&mut service, &[&login]);
    assert_eq!(
        sign(&mut service, &login, [9u8; 32], SESSION_ID, b"user"),
        Err(KmsErrorCode::SshKeyNotFound)
    );
    assert_eq!(snapshot(&mut service, &[&login]), before);
    // Empty store.
    let mut empty = self::service();
    assert_eq!(
        sign(&mut empty, &login, [9u8; 32], SESSION_ID, b"user"),
        Err(KmsErrorCode::SshKeyNotFound)
    );
}

#[test]
fn sign_bad_session_or_username() {
    let (mut service, _admin, login, public) = plain_service();
    let before = snapshot(&mut service, &[&login]);
    let good = sign_request(&login, public, SESSION_ID, b"user").encode();
    let tampered = |at: usize, value: u8| {
        let mut payload = good;
        payload[at] = value;
        payload
    };
    let mut bad: Vec<[u8; 104]> = vec![
        tampered(72, 0),      // username_len 0
        tampered(72, 32),     // username_len 32
        tampered(73, b' '),   // charset
        tampered(73 + 10, 1), // padding after the name
    ];
    let mut zero_session = good;
    zero_session[40..72].fill(0);
    bad.push(zero_session);
    let mut zero_id = good;
    zero_id[..8].fill(0);
    bad.push(zero_id);
    for payload in bad {
        assert_eq!(
            service.raw(KmsOpcode::SignSshUserauth, &payload, &login),
            Err(KmsErrorCode::InvalidRequest)
        );
    }
    assert_eq!(
        service.raw(KmsOpcode::SignSshUserauth, &good[..103], &login),
        Err(KmsErrorCode::InvalidRequest)
    );
    assert_eq!(snapshot(&mut service, &[&login]), before);
}

#[test]
fn wrapped_without_secret() {
    let (mut service, _admin, login, public) = wrapped_service(b"pass", 1);
    let before = snapshot(&mut service, &[&login]);
    let kdf = kdf_calls();
    assert_eq!(
        sign(&mut service, &login, public, SESSION_ID, b"u"),
        Err(KmsErrorCode::SshUnlockRequired)
    );
    assert_eq!(snapshot(&mut service, &[&login]), before);
    assert_eq!(kdf_calls(), kdf);
}

#[test]
fn unlock_begin_bounds_and_plain() {
    let (mut service, _admin, login, public) = wrapped_service(b"pass", 1);
    let before = snapshot(&mut service, &[&login]);
    // secret_len 0 and 1,025, nonzero reserved, wrong length: decode refusals.
    let good = SshUnlockBeginRequestPayload {
        request_id: 1,
        public_key: public,
        secret_len: 4,
        reserved0: 0,
        reserved1: 0,
    }
    .encode();
    for (at, value) in [(40usize, 0u8), (42, 1), (44, 1)] {
        let mut payload = good;
        payload[at] = value;
        if at == 40 {
            payload[41] = 0;
        }
        assert_eq!(
            service.raw(KmsOpcode::BeginSshUnlock, &payload, &login),
            Err(KmsErrorCode::InvalidRequest)
        );
    }
    let mut payload = good;
    payload[40..42].copy_from_slice(&1025u16.to_le_bytes());
    assert_eq!(
        service.raw(KmsOpcode::BeginSshUnlock, &payload, &login),
        Err(KmsErrorCode::InvalidRequest)
    );
    // Unknown key.
    assert_eq!(
        unlock_begin(&mut service, &login, [9u8; 32], 4),
        Err(KmsErrorCode::SshKeyNotFound)
    );
    assert_eq!(snapshot(&mut service, &[&login]), before);
    // The maximum secret is accepted: 1,024 bytes = 11 chunks.
    let begin = unlock_begin(&mut service, &login, public, 1024).unwrap();
    assert_eq!((begin.chunk_count, begin.total_len), (11, 1024));
    unlock_abort(&mut service, &login, begin.handle).unwrap();

    // A plain key never takes an unlock.
    let (mut plain, _admin, login, public) = plain_service();
    let before = snapshot(&mut plain, &[&login]);
    assert_eq!(
        unlock_begin(&mut plain, &login, public, 4),
        Err(KmsErrorCode::InvalidRequest)
    );
    assert_eq!(snapshot(&mut plain, &[&login]), before);
}

#[test]
fn unlock_chunk_violations() {
    let (_, _, login, public) = wrapped_service(b"pass", 1);
    let foreign = Actor::new(201, 9, 21, CALLER_FLAG_KMS_SSH_SIGN);
    let sibling = Actor::new(login.cell, login.generation, 22, login.flags);
    type Send = fn(&mut KmsService, &Actor, &Actor, &Actor, u64) -> Result<(), KmsErrorCode>;
    let cases: Vec<(&str, Send, KmsErrorCode)> = vec![
        (
            "wrong handle",
            |s, a, _, _, h| unlock_chunk(s, a, h ^ 2, 0, &[1u8; 96]).map(drop),
            KmsErrorCode::SshHandleInvalid,
        ),
        (
            "zero handle",
            |s, a, _, _, _| unlock_chunk(s, a, 0, 0, &[1u8; 96]).map(drop),
            KmsErrorCode::SshHandleInvalid,
        ),
        (
            "other principal",
            |s, _, f, _, h| unlock_chunk(s, f, h, 0, &[1u8; 96]).map(drop),
            KmsErrorCode::SshHandleInvalid,
        ),
        (
            "other task",
            |s, _, _, t, h| unlock_chunk(s, t, h, 0, &[1u8; 96]).map(drop),
            KmsErrorCode::SshHandleInvalid,
        ),
        (
            "skipped index",
            |s, a, _, _, h| unlock_chunk(s, a, h, 1, &[1u8; 96]).map(drop),
            KmsErrorCode::SshOrderInvalid,
        ),
        (
            "short non-last",
            |s, a, _, _, h| unlock_chunk(s, a, h, 0, &[1u8; 95]).map(drop),
            KmsErrorCode::SshOrderInvalid,
        ),
        (
            "repeat",
            |s, a, _, _, h| {
                unlock_chunk(s, a, h, 0, &[1u8; 96])?;
                unlock_chunk(s, a, h, 0, &[1u8; 96]).map(drop)
            },
            KmsErrorCode::SshOrderInvalid,
        ),
    ];
    for (label, send, expect) in cases {
        let (mut service, _admin, login, _) = wrapped_service(b"pass", 1);
        let begin = unlock_begin(&mut service, &login, public, 200).unwrap();
        assert_eq!(
            send(&mut service, &login, &foreign, &sibling, begin.handle),
            Err(expect),
            "{label}"
        );
        assert!(matches!(service.ssh.unlock, UnlockSlot::Empty), "{label}");
        assert!(service.ssh.secret.iter().all(|byte| *byte == 0), "{label}");
    }
    // A chunk with no slot, and after Staged.
    let (mut service, _admin, login, _) = wrapped_service(b"pass", 1);
    assert_eq!(
        unlock_chunk(&mut service, &login, 5, 0, &[1u8; 4]),
        Err(KmsErrorCode::SshHandleInvalid)
    );
    let handle = stage_secret(&mut service, &login, public, b"pass").unwrap();
    assert!(matches!(service.ssh.unlock, UnlockSlot::Staged(_)));
    assert_eq!(
        unlock_chunk(&mut service, &login, handle, 1, &[1u8; 4]),
        Err(KmsErrorCode::SshHandleInvalid)
    );
    assert!(matches!(service.ssh.unlock, UnlockSlot::Empty));
}

#[test]
fn foreign_staged_secret_destroyed() {
    let (mut service, _admin, login, public) = wrapped_service(b"pass", 1);
    let other = Actor::new(201, 9, 21, CALLER_FLAG_KMS_SSH_SIGN);
    stage_secret(&mut service, &login, public, b"pass").unwrap();
    let kdf = kdf_calls();
    // Another Login principal signs: refused and the slot is destroyed.
    assert_eq!(
        sign(&mut service, &other, public, SESSION_ID, b"u"),
        Err(KmsErrorCode::SshUnlockRequired)
    );
    assert!(matches!(service.ssh.unlock, UnlockSlot::Empty));
    assert!(service.ssh.secret.iter().all(|byte| *byte == 0));
    assert_eq!(
        sign(&mut service, &login, public, SESSION_ID, b"u"),
        Err(KmsErrorCode::SshUnlockRequired)
    );
    // A Collecting slot is not usable and is destroyed by the attempt.
    let begin = unlock_begin(&mut service, &login, public, 4).unwrap();
    unlock_chunk(&mut service, &login, begin.handle, 0, b"pa").unwrap_err();
    assert!(
        matches!(service.ssh.unlock, UnlockSlot::Empty),
        "short chunk destroys"
    );
    let begin = unlock_begin(&mut service, &login, public, 200).unwrap();
    unlock_chunk(&mut service, &login, begin.handle, 0, &[1u8; 96]).unwrap();
    assert!(matches!(service.ssh.unlock, UnlockSlot::Collecting(_)));
    assert_eq!(
        sign(&mut service, &login, public, SESSION_ID, b"u"),
        Err(KmsErrorCode::SshUnlockRequired)
    );
    assert!(matches!(service.ssh.unlock, UnlockSlot::Empty));
    assert_eq!(kdf_calls(), kdf, "no KDF for any of these");
}

#[test]
fn wrapped_key_end_to_end_with_the_openssh_fixture() {
    // `ssh-keygen -a 16` container, passphrase `hunter42`: same key as the
    // plain fixture, so the signature must equal the plain key's.
    let mut service = service();
    let admin = Actor::admin();
    let login = Actor::login();
    let status = import_blob(&mut service, &admin, b"enc", &id_ed25519_enc()).unwrap();
    assert_eq!(status.custody, KmsSshKeyCustody::PassphraseWrapped);
    assert_eq!(status.public_key, id_ed25519_public());
    stage_secret(&mut service, &login, id_ed25519_public(), PASSPHRASE).unwrap();
    let signed = sign(
        &mut service,
        &login,
        id_ed25519_public(),
        SESSION_ID,
        b"user",
    )
    .unwrap();
    assert_eq!(kdf_calls(), 1);

    let mut plain = self::service();
    import_blob(&mut plain, &admin, b"pk", &id_ed25519()).unwrap();
    let expected = sign(&mut plain, &login, id_ed25519_public(), SESSION_ID, b"user").unwrap();
    assert_eq!(signed.signature, expected.signature);
    assert_eq!(signed.message_sha256, expected.message_sha256);
}

#[test]
fn passphrase_boundaries_and_slot_consumption() {
    for pass in [
        vec![b'a'],
        vec![b'x'; 1024],
        b"pass phrase \xff\x00 ok".to_vec(),
    ] {
        let (mut service, _admin, login, public) = wrapped_service(&pass, 1);
        stage_secret(&mut service, &login, public, &pass).unwrap();
        let first = sign(&mut service, &login, public, SESSION_ID, b"user");
        assert!(first.is_ok(), "len {}", pass.len());
        // Consumed by the one signature: the next sign needs a fresh unlock.
        assert!(matches!(service.ssh.unlock, UnlockSlot::Empty));
        assert_eq!(
            sign(&mut service, &login, public, SESSION_ID, b"user"),
            Err(KmsErrorCode::SshUnlockRequired)
        );
        assert!(service.ssh.secret.iter().all(|byte| *byte == 0));
    }
}

#[test]
fn wrong_passphrase_uniform_failure() {
    let pass = b"right";
    let seed = RFC8032_SEED_1;
    let login = Actor::login();
    let admin = Actor::admin();
    let public = public_of(&seed);
    let session = SESSION_ID;

    // Three failure causes: wrong passphrase, corrupted ciphertext, and a
    // container whose inner public key differs from the outer one. Each must
    // produce the same reply bytes and bump the counter by one.
    let causes: Vec<(&str, Vec<u8>, &[u8])> = {
        let good = Craft::wrapped(&seed, pass, &[0x11; 16], 1);
        let mut corrupt = good.clone();
        corrupt.private[20] ^= 0x40;
        let mut mismatch = Craft::wrapped(&seed, pass, &[0x11; 16], 1);
        // Re-encrypt an inner section for a different seed under the outer key.
        let other = [0x42u8; 32];
        let inner = inner_section(&other, b"x", 16, Pad::Natural);
        mismatch.private = wrap_inner(&inner, pass, &[0x11; 16], 1);
        vec![
            ("wrong passphrase", good.bytes(), &b"wrong"[..]),
            ("corrupt ciphertext", corrupt.bytes(), &pass[..]),
            ("public key mismatch", mismatch.bytes(), &pass[..]),
        ]
    };
    let mut replies: Vec<[u8; 128]> = Vec::new();
    for (label, blob, secret) in causes {
        let mut service = service();
        import_blob(&mut service, &admin, b"wk", &blob).unwrap();
        assert_eq!(service.ssh.failures, 0);
        stage_secret(&mut service, &login, public, secret).unwrap();
        let before = kdf_calls();
        let before_floor = service.ssh.floors.floor(login.principal());
        let payload = sign_request(&login, public, session, b"user").encode();
        let reply = service.frame(KmsOpcode::SignSshUserauth, &payload, &login);
        assert_eq!(kdf_calls(), before + 1, "{label}");
        assert_eq!(service.ssh.failures, 1, "{label}");
        assert!(matches!(service.ssh.unlock, UnlockSlot::Empty), "{label}");
        assert_eq!(
            service.ssh.floors.floor(login.principal()),
            before_floor,
            "failure leaves the floor"
        );
        let response = types::kms::KmsResponseV1::from_bytes(&reply).unwrap();
        assert_eq!(
            response.error_code().unwrap(),
            Some(KmsErrorCode::SshUnlockFailed),
            "{label}"
        );
        replies.push(reply);
    }
    // Byte-identical modulo nothing: same request id, same code.
    assert_eq!(replies[0], replies[1]);
    assert_eq!(replies[0], replies[2]);
}

#[test]
fn limiter_locks_without_kdf() {
    let (mut service, admin, login, public) = wrapped_service(b"right", 1);
    for attempt in 1..=5u8 {
        stage_secret(&mut service, &login, public, b"wrong").unwrap();
        assert_eq!(
            sign(&mut service, &login, public, SESSION_ID, b"u"),
            Err(KmsErrorCode::SshUnlockFailed)
        );
        assert_eq!(service.ssh.failures, attempt);
    }
    // The sixth try is refused at Begin, before any work.
    let kdf = kdf_calls();
    let before = snapshot(&mut service, &[&login]);
    assert_eq!(
        unlock_begin(&mut service, &login, public, 5),
        Err(KmsErrorCode::SshUnlockLocked)
    );
    assert_eq!(snapshot(&mut service, &[&login]), before);
    // A slot that was staged before the limit was reached is refused at Sign
    // before any KDF, and destroyed.
    service.ssh.failures = 4;
    stage_secret(&mut service, &login, public, b"right").unwrap();
    service.ssh.failures = 5;
    assert_eq!(
        sign(&mut service, &login, public, SESSION_ID, b"u"),
        Err(KmsErrorCode::SshUnlockLocked)
    );
    assert!(matches!(service.ssh.unlock, UnlockSlot::Empty));
    assert_eq!(kdf_calls(), kdf, "no KDF after the limit");
    // Reset only by delete + re-import (or a restart).
    delete_key(&mut service, &admin, b"wk", public).unwrap();
    assert_eq!(service.ssh.failures, 0);
    let blob = Craft::wrapped(&RFC8032_SEED_1, b"right", &[0x11; 16], 1).bytes();
    import_blob(&mut service, &admin, b"wk", &blob).unwrap();
    stage_secret(&mut service, &login, public, b"right").unwrap();
    sign(&mut service, &login, public, SESSION_ID, b"u").unwrap();
    // A restart (new service) also starts clean.
    let (mut fresh, _a, login, public) = wrapped_service(b"right", 1);
    assert_eq!(fresh.ssh.failures, 0);
    stage_secret(&mut fresh, &login, public, b"right").unwrap();
}

#[test]
fn a_successful_sign_resets_the_failure_counter() {
    let (mut service, _admin, login, public) = wrapped_service(b"right", 1);
    for _ in 0..4 {
        stage_secret(&mut service, &login, public, b"bad").unwrap();
        sign(&mut service, &login, public, SESSION_ID, b"u").unwrap_err();
    }
    assert_eq!(service.ssh.failures, 4);
    stage_secret(&mut service, &login, public, b"right").unwrap();
    sign(&mut service, &login, public, SESSION_ID, b"u").unwrap();
    assert_eq!(service.ssh.failures, 0);
}

#[test]
fn self_verify_fault_injection() {
    let (mut service, _admin, login, public) = plain_service();
    FLIP_SIGNATURE.with(|flip| flip.set(true));
    let before = snapshot(&mut service, &[&login]);
    let payload = sign_request(&login, public, SESSION_ID, b"user").encode();
    let reply = service.frame(KmsOpcode::SignSshUserauth, &payload, &login);
    FLIP_SIGNATURE.with(|flip| flip.set(false));
    let response = types::kms::KmsResponseV1::from_bytes(&reply).unwrap();
    assert_eq!(
        response.error_code().unwrap(),
        Some(KmsErrorCode::InvalidSignature)
    );
    assert!(response.payload().unwrap().is_empty(), "no signature bytes");
    assert_eq!(
        snapshot(&mut service, &[&login]),
        before,
        "floor and counter untouched"
    );
    // The very next honest request (same service) signs.
    sign(&mut service, &login, public, SESSION_ID, b"user").unwrap();

    // Wrapped key: the slot is consumed, the counter does not move.
    let (mut service, _admin, login, public) = wrapped_service(b"pw", 1);
    stage_secret(&mut service, &login, public, b"pw").unwrap();
    FLIP_SIGNATURE.with(|flip| flip.set(true));
    let result = sign(&mut service, &login, public, SESSION_ID, b"user");
    FLIP_SIGNATURE.with(|flip| flip.set(false));
    assert_eq!(result, Err(KmsErrorCode::InvalidSignature));
    assert_eq!(service.ssh.failures, 0);
    assert!(matches!(service.ssh.unlock, UnlockSlot::Empty));
}

#[test]
fn unlock_replacement_abort_and_delete() {
    let (mut service, admin, login, public) = wrapped_service(b"pass", 1);
    let other = Actor::new(201, 9, 21, CALLER_FLAG_KMS_SSH_SIGN);
    // A second Begin from the same principal replaces and wipes the first.
    let first = unlock_begin(&mut service, &login, public, 200).unwrap();
    unlock_chunk(&mut service, &login, first.handle, 0, &[0x5A; 96]).unwrap();
    assert!(service.ssh.secret.iter().any(|byte| *byte != 0));
    let second = unlock_begin(&mut service, &login, public, 8).unwrap();
    assert_ne!(first.handle, second.handle);
    assert!(service.ssh.secret.iter().all(|byte| *byte == 0));
    // Another principal's Begin replaces it too (no wedge by a dead client).
    let third = unlock_begin(&mut service, &other, public, 8).unwrap();
    assert_eq!(
        unlock_chunk(&mut service, &login, second.handle, 0, &[1u8; 8]),
        Err(KmsErrorCode::SshHandleInvalid)
    );
    let _ = third;
    // Abort: wrong handle and foreign principal leave the slot; the owner wipes it.
    let begin = unlock_begin(&mut service, &login, public, 8).unwrap();
    let before = snapshot(&mut service, &[&login]);
    assert_eq!(
        unlock_abort(&mut service, &login, begin.handle ^ 2),
        Err(KmsErrorCode::SshHandleInvalid)
    );
    assert_eq!(
        unlock_abort(&mut service, &other, begin.handle),
        Err(KmsErrorCode::SshHandleInvalid)
    );
    assert_eq!(snapshot(&mut service, &[&login]), before);
    unlock_abort(&mut service, &login, begin.handle).unwrap();
    assert!(matches!(service.ssh.unlock, UnlockSlot::Empty));
    assert_eq!(
        unlock_abort(&mut service, &login, begin.handle),
        Err(KmsErrorCode::SshHandleInvalid)
    );
    // Delete destroys a staged secret.
    stage_secret(&mut service, &login, public, b"pass").unwrap();
    delete_key(&mut service, &admin, b"wk", public).unwrap();
    assert!(matches!(service.ssh.unlock, UnlockSlot::Empty));
    assert!(service.ssh.secret.iter().all(|byte| *byte == 0));
}

#[test]
fn request_id_replays_are_refused_after_success() {
    let (mut service, _admin, login, public) = plain_service();
    let payload = sign_request(&login, public, SESSION_ID, b"user").encode();
    service
        .raw(KmsOpcode::SignSshUserauth, &payload, &login)
        .unwrap();
    assert_eq!(
        service.raw(KmsOpcode::SignSshUserauth, &payload, &login),
        Err(KmsErrorCode::InvalidRequest),
        "same id replayed"
    );
}
