//! The RFC 4252 section 7 input an `ssh-ed25519` publickey signature covers.
//!
//! This is the client's own construction of the bytes KMS reconstructs from
//! `(session id, username, stored key)` (`k2-cp1.md` 1.4): `string session_id | byte 50 |
//! string user | string "ssh-connection" | string "publickey" | boolean TRUE |
//! string "ssh-ed25519" | string pubkey_blob`, 143 + n bytes for an n-byte user name.
//! Sunset serialises the same message itself ([`crate::sunset::AuthSigMsg`]); the session
//! compares the two byte for byte before anything is signed, so a disagreement between
//! the three implementations (Sunset, this one, KMS) stops the login rather than sending
//! a signature the server would refuse.
use heapless::Vec;
use sha2::{Digest, Sha256};

use crate::sunset::ed25519_dalek::{Signature, VerifyingKey};

/// Longest user name KMS signs for (`KMS_SSH_USERNAME_MAX`).
pub const MAX_USERNAME: usize = 31;
/// Longest signing input: 143 + [`MAX_USERNAME`].
pub const MAX_INPUT: usize = 143 + MAX_USERNAME;

const SERVICE: &[u8] = b"ssh-connection";
const METHOD: &[u8] = b"publickey";
const ALGORITHM: &[u8] = b"ssh-ed25519";
/// `SSH_MSG_USERAUTH_REQUEST`.
const USERAUTH_REQUEST: u8 = 50;

fn string(out: &mut Vec<u8, MAX_INPUT>, bytes: &[u8]) -> Option<()> {
    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes())
        .ok()?;
    out.extend_from_slice(bytes).ok()
}

/// The signing input for `user` over `session_id` with the ed25519 key `public_key`, or
/// `None` for a user name of 0 or more than [`MAX_USERNAME`] bytes.
pub fn signing_input(
    session_id: &[u8; 32],
    user: &str,
    public_key: &[u8; 32],
) -> Option<Vec<u8, MAX_INPUT>> {
    if user.is_empty() || user.len() > MAX_USERNAME {
        return None;
    }
    let mut out = Vec::new();
    string(&mut out, session_id)?;
    out.push(USERAUTH_REQUEST).ok()?;
    string(&mut out, user.as_bytes())?;
    string(&mut out, SERVICE)?;
    string(&mut out, METHOD)?;
    out.push(1).ok()?;
    string(&mut out, ALGORITHM)?;
    // The 51-byte public key blob: string "ssh-ed25519" | string key.
    out.extend_from_slice(&(4 + ALGORITHM.len() as u32 + 4 + 32).to_be_bytes())
        .ok()?;
    string(&mut out, ALGORITHM)?;
    string(&mut out, public_key)?;
    Some(out)
}

/// SHA-256 of a signing input (the digest KMS returns beside the signature).
pub fn digest(message: &[u8]) -> [u8; 32] {
    Sha256::digest(message).into()
}

/// Whether `signature` (`R || S`) is a valid ed25519 signature of `message` under
/// `public_key`. Strict: a malleable or small-order form is refused.
pub fn verify(public_key: &[u8; 32], message: &[u8], signature: &[u8; 64]) -> bool {
    match VerifyingKey::from_bytes(public_key) {
        Ok(key) => key
            .verify_strict(message, &Signature::from_bytes(signature))
            .is_ok(),
        Err(_) => false,
    }
}
