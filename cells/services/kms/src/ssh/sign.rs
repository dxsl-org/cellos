//! RFC 4252 section 7 userauth input reconstruction, ed25519 signing, and the
//! mandatory self-verification before any signature leaves the service.

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use types::kms::{KMS_SSH_PUBLIC_KEY_LEN, KMS_SSH_SESSION_ID_LEN, KMS_SSH_SIGNATURE_LEN};

use super::limits::{SIGN_INPUT_FIXED, SIGN_INPUT_MAX};

#[cfg(test)]
std::thread_local! {
    /// Test seam: flip one signature byte before the self-verification.
    pub(crate) static FLIP_SIGNATURE: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
}

/// The signature did not verify against the stored public key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SelfVerifyFailed;

fn put(out: &mut [u8; SIGN_INPUT_MAX], at: &mut usize, bytes: &[u8]) {
    if let Some(end) = at.checked_add(bytes.len()) {
        if let Some(dst) = out.get_mut(*at..end) {
            dst.copy_from_slice(bytes);
            *at = end;
        }
    }
}

/// Assemble `string session_id | byte 50 | string user | string
/// "ssh-connection" | string "publickey" | boolean TRUE | string "ssh-ed25519"
/// | string pubkey_blob` into `out` and return its length (`143 + n`).
///
/// `username` is already validated (1..=31 bytes), so the length is at most
/// [`SIGN_INPUT_MAX`]; an over-long username leaves a short buffer and a
/// length the caller never signs (it is rejected at decode).
pub(crate) fn reconstruct(
    session_id: &[u8; KMS_SSH_SESSION_ID_LEN],
    username: &[u8],
    public_key: &[u8; KMS_SSH_PUBLIC_KEY_LEN],
    out: &mut [u8; SIGN_INPUT_MAX],
) -> usize {
    let mut at = 0usize;
    put(out, &mut at, &32u32.to_be_bytes());
    put(out, &mut at, session_id);
    put(out, &mut at, &[50]);
    put(out, &mut at, &(username.len() as u32).to_be_bytes());
    put(out, &mut at, username);
    put(out, &mut at, &14u32.to_be_bytes());
    put(out, &mut at, b"ssh-connection");
    put(out, &mut at, &9u32.to_be_bytes());
    put(out, &mut at, b"publickey");
    put(out, &mut at, &[1]);
    put(out, &mut at, &11u32.to_be_bytes());
    put(out, &mut at, b"ssh-ed25519");
    put(out, &mut at, &51u32.to_be_bytes());
    put(out, &mut at, &11u32.to_be_bytes());
    put(out, &mut at, b"ssh-ed25519");
    put(out, &mut at, &32u32.to_be_bytes());
    put(out, &mut at, public_key);
    debug_assert_eq!(at, SIGN_INPUT_FIXED + username.len());
    at
}

/// Sign `input` with `seed` and verify (strictly) against the stored public
/// key before returning. `SigningKey` zeroizes its seed and expanded key on
/// drop.
pub(crate) fn sign(
    seed: &[u8; 32],
    input: &[u8],
    public_key: &[u8; KMS_SSH_PUBLIC_KEY_LEN],
) -> Result<[u8; KMS_SSH_SIGNATURE_LEN], SelfVerifyFailed> {
    let key = SigningKey::from_bytes(seed);
    #[allow(unused_mut)]
    let mut bytes = key.sign(input).to_bytes();
    #[cfg(test)]
    if FLIP_SIGNATURE.with(|flip| flip.get()) {
        bytes[0] ^= 0x01;
    }
    let verifying = VerifyingKey::from_bytes(public_key).map_err(|_| SelfVerifyFailed)?;
    verifying
        .verify_strict(input, &Signature::from_bytes(&bytes))
        .map_err(|_| SelfVerifyFailed)?;
    Ok(bytes)
}
