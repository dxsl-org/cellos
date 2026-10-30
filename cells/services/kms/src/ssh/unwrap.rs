//! Seed recovery: plain inner sections at import, passphrase-wrapped sections
//! at sign time (bcrypt KDF, AES-256-CTR, inner validation, public-key check).

use aes::cipher::{KeyIvInit, StreamCipher};
use aes::Aes256;
use bcrypt_pbkdf::bcrypt_pbkdf_with_memory;
use ctr::Ctr128BE;
use ed25519_dalek::SigningKey;
use zeroize::Zeroizing;

use super::parser::{parse_inner, Cipher, Reject, STORE_WRAPPED_CT_MAX};

#[cfg(test)]
std::thread_local! {
    /// Number of KDF invocations on this thread (test seam: every pre-KDF
    /// reject and the failure limiter must leave it unchanged).
    pub(crate) static KDF_CALLS: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
}

/// One undifferentiated failure: wrong passphrase, corrupt ciphertext and a
/// mismatching public key must not be distinguishable by a caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UnwrapFailed;

/// I7: the seed derives exactly the public key recorded in the container.
/// Without the `ed25519` crate feature even `ssh-key` skips this check.
pub(crate) fn seed_matches_public(seed: &[u8; 32], public: &[u8; 32]) -> bool {
    SigningKey::from_bytes(seed).verifying_key().as_bytes() == public
}

/// Validate a plaintext inner section (I1-I7) and return the seed.
pub(crate) fn plain_seed(
    inner: &[u8],
    cipher: Cipher,
    public: &[u8; 32],
) -> Result<Zeroizing<[u8; 32]>, Reject> {
    let mut seed = Zeroizing::new([0u8; 32]);
    parse_inner(inner, cipher, public, &mut seed)?;
    if !seed_matches_public(&seed, public) {
        return Err(Reject::PublicMismatch);
    }
    Ok(seed)
}

/// Run the KDF, decrypt, validate, and return the seed. All intermediates
/// (derived key, KDF scratch, plaintext section) are wiped on every path.
pub(crate) fn unwrap_seed(
    passphrase: &[u8],
    salt: &[u8],
    rounds: u32,
    ciphertext: &[u8],
    public: &[u8; 32],
) -> Result<Zeroizing<[u8; 32]>, UnwrapFailed> {
    let mut plain = Zeroizing::new([0u8; STORE_WRAPPED_CT_MAX]);
    plain
        .get_mut(..ciphertext.len())
        .ok_or(UnwrapFailed)?
        .copy_from_slice(ciphertext);
    // 48 output bytes need ceil(48 / 32) * 32 = 64 bytes of KDF memory.
    let mut okm = Zeroizing::new([0u8; 64]);
    let mut memory = Zeroizing::new([0u8; 64]);
    #[cfg(test)]
    KDF_CALLS.with(|calls| calls.set(calls.get() + 1));
    {
        let output = okm.get_mut(..48).ok_or(UnwrapFailed)?;
        bcrypt_pbkdf_with_memory(passphrase, salt, rounds, output, &mut memory[..])
            .map_err(|_| UnwrapFailed)?;
    }
    let (key, iv) = okm
        .get(..48)
        .and_then(|derived| derived.split_at_checked(32))
        .ok_or(UnwrapFailed)?;
    let mut cipher = Ctr128BE::<Aes256>::new_from_slices(key, iv).map_err(|_| UnwrapFailed)?;
    let section = plain.get_mut(..ciphertext.len()).ok_or(UnwrapFailed)?;
    cipher.apply_keystream(section);
    plain_seed(section, Cipher::Aes256Ctr, public).map_err(|_| UnwrapFailed)
}
