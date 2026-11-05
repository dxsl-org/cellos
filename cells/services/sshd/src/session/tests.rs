//! Consumer-visible session contract: bounded inputs and phase enforcement
//! (authentication itself: `tests/auth.rs`).

use self::auth::signed_request;
use super::*;
use crate::ssh_packet::{SSH_MSG_CHANNEL_OPEN, SSH_MSG_CHANNEL_REQUEST};
use alloc::vec;
use ed25519_compact::Seed;

const SESSION_ID: [u8; 32] = [0x5a; 32];

fn counting_entropy(buf: &mut [u8]) -> bool {
    for (i, b) in buf.iter_mut().enumerate() {
        *b = (i as u8).wrapping_mul(37).wrapping_add(11);
    }
    true
}

fn no_entropy(_: &mut [u8]) -> bool {
    false
}

fn user_key() -> KeyPair {
    KeyPair::from_seed(Seed::new([0x11; 32]))
}

fn pk(kp: &KeyPair) -> [u8; 32] {
    *kp.pk
}

fn session_in(phase: SessionPhase, authorized: Vec<[u8; 32]>) -> SshSession {
    let host = KeyPair::from_seed(Seed::new([0x22; 32]));
    let mut s = SshSession::new(host, counting_entropy, authorized);
    s.phase = phase;
    s.kex.session_id = SESSION_ID;
    s
}

fn key_blob(key: &[u8; 32]) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_string(b"ssh-ed25519");
    w.write_string(key);
    w.as_slice().to_vec()
}

/// `USERAUTH_REQUEST` for `publickey`; `sig` = None is the query form.
fn pk_request(user: &str, key: &[u8; 32], sig: Option<&[u8]>) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_byte(SSH_MSG_USERAUTH_REQUEST);
    w.write_string(user.as_bytes());
    w.write_string(b"ssh-connection");
    w.write_string(b"publickey");
    w.write_bool(sig.is_some());
    w.write_string(b"ssh-ed25519");
    w.write_string(&key_blob(key));
    if let Some(sig) = sig {
        let mut s = PacketWriter::new();
        s.write_string(b"ssh-ed25519");
        s.write_string(sig);
        w.write_string(s.as_slice());
    }
    w.as_slice().to_vec()
}

fn assert_disconnected(s: &SshSession, replies: &[Vec<u8>], reason: u32) {
    assert_eq!(s.phase, SessionPhase::Closed);
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0][0], SSH_MSG_DISCONNECT);
    assert_eq!(replies[0][1..5], reason.to_be_bytes());
}

#[test]
fn messages_outside_their_phase_disconnect() {
    let kp = user_key();
    let mut open = vec![SSH_MSG_CHANNEL_OPEN];
    open.extend_from_slice(b"\0\0\0\x07session\0\0\0\0\0\0\x80\0\0\0\x80\0");
    let mut exec = vec![SSH_MSG_CHANNEL_REQUEST];
    exec.extend_from_slice(b"\0\0\0\0\0\0\0\x04exec\x01\0\0\0\x02ps");
    let mut service = vec![SSH_MSG_SERVICE_REQUEST];
    service.extend_from_slice(b"\0\0\0\x0essh-connection");
    let cases = [
        (SessionPhase::KexInit, service.clone()),
        (
            SessionPhase::WaitNewKeys,
            signed_request(&kp, &SESSION_ID, "admin"),
        ),
        (
            SessionPhase::ServiceRequest,
            signed_request(&kp, &SESSION_ID, "admin"),
        ),
        (SessionPhase::ServiceRequest, open.clone()),
        (SessionPhase::ServiceRequest, exec.clone()),
        (SessionPhase::UserAuth, open.clone()),
        (SessionPhase::UserAuth, exec.clone()),
        (SessionPhase::UserAuth, service.clone()),
        // Authenticated, but no channel is open yet.
        (SessionPhase::ActiveSession, exec),
    ];
    for (phase, payload) in cases {
        let mut s = session_in(phase, vec![pk(&kp)]);
        let replies = s.dispatch(&payload);
        assert_disconnected(&s, &replies, DISCONNECT_PROTOCOL_ERROR);
    }
    let mut s = session_in(SessionPhase::ServiceRequest, vec![pk(&kp)]);
    let replies = s.dispatch(&service);
    assert_disconnected(&s, &replies, DISCONNECT_SERVICE_NOT_AVAILABLE);
}

#[test]
fn non_contributory_client_key_aborts_kex() {
    let mut s = session_in(SessionPhase::EcdhInit, Vec::new());
    let mut init = vec![SSH_MSG_KEX_ECDH_INIT];
    init.extend_from_slice(&[0, 0, 0, 32]);
    init.extend_from_slice(&[0; 32]);
    let replies = s.dispatch(&init);
    assert_disconnected(&s, &replies, DISCONNECT_KEY_EXCHANGE_FAILED);
}

#[test]
fn version_line_is_bounded_and_entropy_failure_closes() {
    let mut s = session_in(SessionPhase::VersionExchange, Vec::new());
    // The exchange hash covers the banner without its CR LF.
    let mut banner = crate::kex::SERVER_VERSION_RAW.to_vec();
    banner.extend_from_slice(b"\r\n");
    assert_eq!(s.initial_outbound(), banner.as_slice());
    assert!(s.process_incoming(&[b'A'; MAX_VERSION_LINE - 1]).is_empty());
    assert_eq!(s.phase, SessionPhase::VersionExchange);
    s.process_incoming(b"A");
    assert_eq!(s.phase, SessionPhase::Closed);

    let mut s = session_in(SessionPhase::VersionExchange, Vec::new());
    assert!(!s.process_incoming(b"SSH-2.0-OpenSSH_9.6\r\n").is_empty());
    assert_eq!(s.phase, SessionPhase::KexInit);

    let host = KeyPair::from_seed(Seed::new([0x22; 32]));
    let mut s = SshSession::new(host, no_entropy, Vec::new());
    assert!(s.process_incoming(b"SSH-2.0-OpenSSH_9.6\r\n").is_empty());
    assert_eq!(s.phase, SessionPhase::Closed);
}

#[test]
fn oversized_pre_newkeys_packet_disconnects() {
    let mut s = session_in(SessionPhase::KexInit, Vec::new());
    let length = (crate::cipher::MAX_PLAINTEXT_PACKET_LENGTH + 1) as u32;
    let out = s.process_incoming(&length.to_be_bytes());
    assert_eq!(s.phase, SessionPhase::Closed);
    // Plaintext packet: length(4) padding_length(1) payload…
    assert_eq!(out[5], SSH_MSG_DISCONNECT);
}

mod auth;
