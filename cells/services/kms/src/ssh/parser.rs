//! Pure OpenSSH-v1 ed25519 container parser (K5 CP1 section 7).
//!
//! Depends on `core` only so the fuzz crate can include this exact file with
//! `#[path]`. Every access goes through a checked reader; no allocation exists
//! and nothing is ever sized from a declared length. No KDF, no cipher and no
//! hashing of secrets happens here: the outer allowlist (P1-P9) runs before any
//! expensive work, and the inner section checks (I1-I6) run over bytes the
//! caller already holds in plaintext.
//!
//! Reject reasons exist for tests only. The service maps every one of them to
//! the single `SshKeyFormatRejected` reply, except [`Reject::WrappedTooLarge`]
//! which is the store budget (`SshStoreFull`).
#![deny(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

/// Salt bound accepted before the KDF runs (OpenSSH writes 16).
pub(crate) const KDF_SALT_MIN: usize = 16;
pub(crate) const KDF_SALT_MAX: usize = 64;
/// Round bound accepted before the KDF runs (contract: 1..=64).
pub(crate) const KDF_ROUNDS_MAX: u32 = 64;
/// Largest wrapped private section stored (decision D-5.2).
pub(crate) const STORE_WRAPPED_CT_MAX: usize = 2_048;

const MAGIC: &[u8] = b"openssh-key-v1\0";
const KEY_TYPE: &[u8] = b"ssh-ed25519";
/// `u32 checkint x2 + string keytype + string pub + string keypair + string
/// comment` for the shortest valid ed25519 key: 8 + 15 + 36 + 68 + 4.
const INNER_MIN: usize = 131;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reject {
    Truncated,
    Magic,
    Cipher,
    Kdf,
    CipherKdfMismatch,
    KdfOptions,
    Salt,
    Rounds,
    KeyCount,
    PublicKey,
    PrivateLen,
    Trailing,
    /// Wrapped private section larger than [`STORE_WRAPPED_CT_MAX`].
    WrappedTooLarge,
    Checkint,
    KeyType,
    PublicMismatch,
    KeypairLayout,
    Padding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cipher {
    None,
    Aes256Ctr,
}

/// Validated outer container. Slices borrow the input blob.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Outer<'a> {
    pub cipher: Cipher,
    /// Empty for [`Cipher::None`].
    pub salt: &'a [u8],
    /// Zero for [`Cipher::None`].
    pub rounds: u32,
    pub public: [u8; 32],
    /// The private section: plaintext for `None`, ciphertext for `Aes256Ctr`.
    pub private: &'a [u8],
}

struct Reader<'a> {
    buf: &'a [u8],
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf }
    }

    fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    fn remaining(&self) -> usize {
        self.buf.len()
    }

    fn bytes(&mut self, len: usize) -> Result<&'a [u8], Reject> {
        let (head, tail) = self.buf.split_at_checked(len).ok_or(Reject::Truncated)?;
        self.buf = tail;
        Ok(head)
    }

    fn u32(&mut self) -> Result<u32, Reject> {
        let raw = self.bytes(4)?;
        let mut word = [0u8; 4];
        word.copy_from_slice(raw);
        Ok(u32::from_be_bytes(word))
    }

    /// `string`: u32 big-endian length, checked against the remainder before
    /// any slicing.
    fn string(&mut self) -> Result<&'a [u8], Reject> {
        let len = self.u32()?;
        let len = usize::try_from(len).map_err(|_| Reject::Truncated)?;
        self.bytes(len)
    }
}

/// Parse and allowlist the outer container (steps P1-P9). Runs identically for
/// plain and wrapped keys and never performs a KDF.
pub(crate) fn parse_outer(blob: &[u8]) -> Result<Outer<'_>, Reject> {
    let mut reader = Reader::new(blob);
    if reader.bytes(MAGIC.len())? != MAGIC {
        return Err(Reject::Magic);
    }
    let cipher = match reader.string()? {
        b"none" => Cipher::None,
        b"aes256-ctr" => Cipher::Aes256Ctr,
        _ => return Err(Reject::Cipher),
    };
    let kdf_is_bcrypt = match reader.string()? {
        b"none" => false,
        b"bcrypt" => true,
        _ => return Err(Reject::Kdf),
    };
    if kdf_is_bcrypt != (cipher == Cipher::Aes256Ctr) {
        return Err(Reject::CipherKdfMismatch);
    }
    let options = reader.string()?;
    let (salt, rounds) = if kdf_is_bcrypt {
        let mut sub = Reader::new(options);
        let salt = sub.string()?;
        let rounds = sub.u32()?;
        if !sub.is_empty() {
            return Err(Reject::KdfOptions);
        }
        if !(KDF_SALT_MIN..=KDF_SALT_MAX).contains(&salt.len()) {
            return Err(Reject::Salt);
        }
        if !(1..=KDF_ROUNDS_MAX).contains(&rounds) {
            return Err(Reject::Rounds);
        }
        (salt, rounds)
    } else {
        if !options.is_empty() {
            return Err(Reject::KdfOptions);
        }
        (&[][..], 0)
    };
    if reader.u32()? != 1 {
        return Err(Reject::KeyCount);
    }
    let public_blob = reader.string()?;
    let public = {
        let mut sub = Reader::new(public_blob);
        if sub.string()? != KEY_TYPE {
            return Err(Reject::PublicKey);
        }
        let key = sub.string()?;
        if !sub.is_empty() {
            return Err(Reject::PublicKey);
        }
        let mut public = [0u8; 32];
        if key.len() != public.len() {
            return Err(Reject::PublicKey);
        }
        public.copy_from_slice(key);
        public
    };
    let private = reader.string()?;
    match cipher {
        Cipher::None => {
            if private.len() % 8 != 0 || private.len() < padded_min(8) {
                return Err(Reject::PrivateLen);
            }
        }
        Cipher::Aes256Ctr => {
            if private.len() % 16 != 0 || private.len() < padded_min(16) {
                return Err(Reject::PrivateLen);
            }
            if private.len() > STORE_WRAPPED_CT_MAX {
                return Err(Reject::WrappedTooLarge);
            }
        }
    }
    if !reader.is_empty() {
        return Err(Reject::Trailing);
    }
    Ok(Outer {
        cipher,
        salt,
        rounds,
        public,
        private,
    })
}

/// Shortest private section for block size `block`: `INNER_MIN` rounded up.
const fn padded_min(block: usize) -> usize {
    INNER_MIN.div_ceil(block).saturating_mul(block)
}

/// Validate a plaintext inner section (steps I1-I6) and copy the ed25519 seed
/// into `seed`. Deriving the public key from the seed (I7) needs the signer
/// crate and is done by the caller.
///
/// `cipher` selects the padding rule and mirrors `ssh-key` exactly: block size
/// 8 and up to 16 padding bytes for an unencrypted section (PuTTYgen writes
/// 16), block size 16 and up to 15 for an encrypted one.
pub(crate) fn parse_inner(
    plain: &[u8],
    cipher: Cipher,
    public: &[u8; 32],
    seed: &mut [u8; 32],
) -> Result<(), Reject> {
    let max_pad = match cipher {
        Cipher::None => 16,
        Cipher::Aes256Ctr => 15,
    };
    let mut reader = Reader::new(plain);
    let check1 = reader.u32()?;
    let check2 = reader.u32()?;
    if check1 != check2 {
        return Err(Reject::Checkint);
    }
    if reader.string()? != KEY_TYPE {
        return Err(Reject::KeyType);
    }
    if reader.string()? != public.as_slice() {
        return Err(Reject::PublicMismatch);
    }
    let keypair = reader.string()?;
    if keypair.len() != 64 {
        return Err(Reject::KeypairLayout);
    }
    let (secret_half, public_half) = keypair.split_at_checked(32).ok_or(Reject::KeypairLayout)?;
    if public_half != public.as_slice() {
        return Err(Reject::KeypairLayout);
    }
    // The comment is never stored and never returned; only its framing is
    // checked.
    reader.string()?;
    if reader.remaining() > max_pad {
        return Err(Reject::Padding);
    }
    let mut expected = 0u8;
    while !reader.is_empty() {
        expected = expected.checked_add(1).ok_or(Reject::Padding)?;
        let byte = reader.bytes(1)?;
        if byte != [expected] {
            return Err(Reject::Padding);
        }
    }
    seed.copy_from_slice(secret_half);
    Ok(())
}
