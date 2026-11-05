//! publickey verification over the exact RFC 4252 §7 bytes, the query form,
//! and the failure lockout.

use super::*;
use crate::auth::{DISCONNECT_NO_MORE_AUTH_METHODS, MAX_FAILURES};
use crate::ssh_packet::{SSH_MSG_USERAUTH_FAILURE, SSH_MSG_USERAUTH_PK_OK};

/// The RFC 4252 §7 signing input, spelled out field by field.
fn expected_signed_blob(session_id: &[u8; 32], user: &str, key: &[u8; 32]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&[0, 0, 0, 32]);
    v.extend_from_slice(session_id);
    v.push(50);
    v.extend_from_slice(&(user.len() as u32).to_be_bytes());
    v.extend_from_slice(user.as_bytes());
    v.extend_from_slice(b"\0\0\0\x0essh-connection");
    v.extend_from_slice(b"\0\0\0\x09publickey");
    v.push(1);
    v.extend_from_slice(b"\0\0\0\x0bssh-ed25519");
    v.extend_from_slice(b"\0\0\0\x33\0\0\0\x0bssh-ed25519\0\0\0\x20");
    v.extend_from_slice(key);
    v
}

pub(super) fn signed_request(kp: &KeyPair, session_id: &[u8; 32], user: &str) -> Vec<u8> {
    let sig = kp
        .sk
        .sign(expected_signed_blob(session_id, user, &pk(kp)), None);
    pk_request(user, &pk(kp), Some(sig.as_ref()))
}

#[test]
fn signed_blob_is_the_rfc4252_input_and_verifies() {
    let kp = user_key();
    let built = vissh::userauth::signing_input(&SESSION_ID, "admin", &pk(&kp)).unwrap();
    let expected = expected_signed_blob(&SESSION_ID, "admin", &pk(&kp));
    assert_eq!(built.as_slice(), expected.as_slice());
    assert_eq!(expected.len(), 143 + "admin".len());

    let mut s = session_in(SessionPhase::UserAuth, vec![pk(&kp)]);
    let replies = s.dispatch(&signed_request(&kp, &SESSION_ID, "admin"));
    assert_eq!(replies, [vec![SSH_MSG_USERAUTH_SUCCESS]]);
    assert!(s.is_authenticated());
}

#[test]
fn query_form_and_wrong_signatures_fail_without_success() {
    let kp = user_key();
    let other = KeyPair::from_seed(Seed::new([0x33; 32]));
    let mut s = session_in(SessionPhase::UserAuth, vec![pk(&kp)]);
    let ok = s.dispatch(&pk_request("admin", &pk(&kp), None));
    assert_eq!(ok[0][0], SSH_MSG_USERAUTH_PK_OK);
    // Unauthorized key, a signature over another session, and an
    // empty user name are all plain failures naming only `publickey`.
    for request in [
        pk_request("admin", &pk(&other), None),
        signed_request(&kp, &[0x5b; 32], "admin"),
        signed_request(&kp, &SESSION_ID, ""),
    ] {
        let replies = s.dispatch(&request);
        assert_eq!(replies, [crate::auth::failure_payload()]);
        assert_eq!(s.phase, SessionPhase::UserAuth);
    }
    assert_eq!(crate::auth::failure_payload()[0], SSH_MSG_USERAUTH_FAILURE);
    assert_eq!(
        &crate::auth::failure_payload()[1..],
        b"\0\0\0\x09publickey\0"
    );
}

#[test]
fn fifth_failure_disconnects() {
    let kp = user_key();
    let mut s = session_in(SessionPhase::UserAuth, vec![pk(&kp)]);
    let none = {
        let mut w = PacketWriter::new();
        w.write_byte(SSH_MSG_USERAUTH_REQUEST);
        w.write_string(b"admin");
        w.write_string(b"ssh-connection");
        w.write_string(b"none");
        w.as_slice().to_vec()
    };
    // The first `none` probe is free; everything after counts.
    assert_eq!(s.dispatch(&none), [crate::auth::failure_payload()]);
    let bad = signed_request(&kp, &[0; 32], "admin");
    for _ in 1..MAX_FAILURES {
        assert_eq!(s.dispatch(&bad), [crate::auth::failure_payload()]);
    }
    let replies = s.dispatch(&bad);
    assert_disconnected(&s, &replies, DISCONNECT_NO_MORE_AUTH_METHODS);
}
