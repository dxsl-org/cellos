//! RFC 4252 user authentication: `publickey` with `ssh-ed25519` only.
//!
//! - Query form (no signature): `USERAUTH_PK_OK` if the key is authorized,
//!   otherwise a failure.
//! - Signed form: the signature must verify over the RFC 4252 §7 input built by
//!   `vissh::userauth::signing_input` (the same bytes the Cellos ssh client and
//!   KMS sign) against a key listed in `/srv/sshd/authorized_keys`.
//! - `none` and every other method fail; there is no password method.
//!
//! Every failure answers `USERAUTH_FAILURE` naming `publickey` alone with
//! `partial success = false`. The [`MAX_FAILURES`]th failure disconnects
//! instead. Like OpenSSH, the client's first `none` probe is not counted.
//!
//! The user name is checked (non-empty UTF-8, at most [`MAX_USER`] bytes) and
//! signed over, but it is not mapped to an account: this is a single-user
//! device, and any authorized key logs in as that user whatever name it gives.

extern crate alloc;
use alloc::vec::Vec;

use crate::authorized_keys::{ed25519_from_blob, KEY_TYPE};
use crate::ssh_packet::{
    PacketReader, PacketWriter, SSH_MSG_USERAUTH_FAILURE, SSH_MSG_USERAUTH_PK_OK,
};

/// Failures before the connection is dropped.
pub const MAX_FAILURES: u32 = 5;
/// Longest user name: the bound of the shared signing-input builder (and of
/// KMS, which signs for the client), so all three agree on what can be signed.
pub const MAX_USER: usize = vissh::userauth::MAX_USERNAME;
/// The only service a successful login opens.
pub const SERVICE: &str = "ssh-connection";

/// RFC 4253 §11.1 reason codes used by authentication.
pub const DISCONNECT_PROTOCOL_ERROR: u32 = 2;
pub const DISCONNECT_SERVICE_NOT_AVAILABLE: u32 = 7;
pub const DISCONNECT_NO_MORE_AUTH_METHODS: u32 = 14;

#[derive(Debug, PartialEq, Eq)]
pub enum AuthReply {
    /// Send this payload and keep waiting for `USERAUTH_REQUEST`.
    Continue(Vec<u8>),
    /// Authenticated: send `USERAUTH_SUCCESS` and open the connection layer.
    Success,
    /// Send `SSH_MSG_DISCONNECT` with this reason and close.
    Disconnect(u32, &'static str),
}

pub struct Auth {
    keys: Vec<[u8; 32]>,
    failures: u32,
    free_none_used: bool,
}

impl Auth {
    pub fn new(keys: Vec<[u8; 32]>) -> Self {
        Self {
            keys,
            failures: 0,
            free_none_used: false,
        }
    }

    /// Handle one `USERAUTH_REQUEST` body (everything after the message byte).
    pub fn handle(&mut self, session_id: &[u8; 32], body: &[u8]) -> AuthReply {
        let mut r = PacketReader::new(body);
        let (Some(user), Some(service), Some(method)) =
            (r.read_utf8_string(), r.read_utf8_string(), r.read_string())
        else {
            return AuthReply::Disconnect(DISCONNECT_PROTOCOL_ERROR, "malformed userauth request");
        };
        if service != SERVICE {
            return AuthReply::Disconnect(DISCONNECT_SERVICE_NOT_AVAILABLE, "unknown service");
        }
        if method == b"none" && !self.free_none_used {
            self.free_none_used = true;
            return AuthReply::Continue(failure_payload());
        }
        if user.is_empty() || user.len() > MAX_USER || method != b"publickey" {
            return self.fail();
        }
        let (Some(signed), Some(algorithm), Some(blob)) =
            (r.read_bool(), r.read_string(), r.read_string())
        else {
            return AuthReply::Disconnect(DISCONNECT_PROTOCOL_ERROR, "malformed publickey request");
        };
        let key = match ed25519_from_blob(blob) {
            Some(key) if algorithm == KEY_TYPE && self.keys.contains(&key) => key,
            _ => return self.fail(),
        };
        if !signed {
            if !r.is_at_end() {
                return AuthReply::Disconnect(DISCONNECT_PROTOCOL_ERROR, "trailing data");
            }
            let mut w = PacketWriter::new();
            w.write_byte(SSH_MSG_USERAUTH_PK_OK);
            w.write_string(algorithm);
            w.write_string(blob);
            return AuthReply::Continue(w.as_slice().to_vec());
        }
        let Some(signature) = r.read_string().filter(|_| r.is_at_end()) else {
            return AuthReply::Disconnect(DISCONNECT_PROTOCOL_ERROR, "malformed signature");
        };
        match (
            signature_bytes(signature),
            vissh::userauth::signing_input(session_id, user, &key),
        ) {
            (Some(sig), Some(input)) if vissh::userauth::verify(&key, &input, &sig) => {
                AuthReply::Success
            }
            _ => self.fail(),
        }
    }

    fn fail(&mut self) -> AuthReply {
        self.failures += 1;
        if self.failures >= MAX_FAILURES {
            AuthReply::Disconnect(
                DISCONNECT_NO_MORE_AUTH_METHODS,
                "too many authentication failures",
            )
        } else {
            AuthReply::Continue(failure_payload())
        }
    }
}

/// The 64 signature bytes of `string "ssh-ed25519" || string sig`.
fn signature_bytes(blob: &[u8]) -> Option<[u8; 64]> {
    let mut r = PacketReader::new(blob);
    if r.read_string()? != KEY_TYPE {
        return None;
    }
    let sig: [u8; 64] = r.read_string()?.try_into().ok()?;
    r.is_at_end().then_some(sig)
}

/// `USERAUTH_FAILURE`: `publickey` is the only method that can continue;
/// partial success is never claimed.
pub fn failure_payload() -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_byte(SSH_MSG_USERAUTH_FAILURE);
    w.write_name_list(&["publickey"]);
    w.write_bool(false);
    w.as_slice().to_vec()
}
