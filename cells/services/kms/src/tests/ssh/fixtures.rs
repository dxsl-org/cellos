//! Fixture access and hand-crafted OpenSSH containers (never from
//! `ssh-keygen`: the `-a` rounds, salt length and cipher set it can produce
//! are narrower than the boundary cases the parser must refuse).

use std::vec::Vec;

use aes::cipher::{KeyIvInit, StreamCipher};
use aes::Aes256;
use bcrypt_pbkdf::bcrypt_pbkdf_with_memory;
use ctr::Ctr128BE;
use ed25519_dalek::SigningKey;

mod data;

pub(super) const PASSPHRASE: &[u8] = b"hunter42";

/// RFC 8032 section 7.1 TEST 1 secret key: the crafted containers' seed.
pub(super) const RFC8032_SEED_1: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];

pub(super) fn hex(text: &str) -> Vec<u8> {
    assert_eq!(text.len() % 2, 0);
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).unwrap())
        .collect()
}

pub(super) fn hex32(text: &str) -> [u8; 32] {
    hex(text).try_into().unwrap()
}

pub(super) fn id_ed25519() -> Vec<u8> {
    hex(data::ID_ED25519)
}
pub(super) fn id_ed25519_enc() -> Vec<u8> {
    hex(data::ID_ED25519_AES256_CTR_ENC)
}
pub(super) fn puttygen_overpadded() -> Vec<u8> {
    hex(data::PUTTYGEN_OVERPADDED)
}

/// Public key of `id_ed25519` and of its wrapped twin.
pub(super) fn id_ed25519_public() -> [u8; 32] {
    hex32("b33eaef37ea2df7caa010defdea34e241f65f1b529a4f43ed14327f5c54aab62")
}

/// Every public container that must be refused at the structural allowlist,
/// with the reason it is not an `ssh-ed25519` + `none`/`aes256-ctr`+`bcrypt`
/// container.
pub(super) fn foreign_fixtures() -> Vec<(&'static str, Vec<u8>)> {
    std::vec![
        ("aes128-ctr", hex(data::ID_ED25519_AES128_CTR_ENC)),
        ("aes192-ctr", hex(data::ID_ED25519_AES192_CTR_ENC)),
        ("aes128-cbc", hex(data::ID_ED25519_AES128_CBC_ENC)),
        ("aes192-cbc", hex(data::ID_ED25519_AES192_CBC_ENC)),
        ("aes256-cbc", hex(data::ID_ED25519_AES256_CBC_ENC)),
        ("3des-cbc", hex(data::ID_ED25519_3DES_CBC_ENC)),
        (
            "chacha20-poly1305",
            hex(data::ID_ED25519_CHACHA20_POLY1305_ENC),
        ),
        ("aes128-gcm", hex(data::ID_ED25519_AES128_GCM_ENC)),
        ("aes256-gcm", hex(data::ID_ED25519_AES256_GCM_ENC)),
        ("ecdsa-p256", hex(data::ID_ECDSA_P256)),
        ("rsa-3072", hex(data::ID_RSA_3072)),
        ("dsa-1024", hex(data::ID_DSA_1024)),
        ("opaque", hex(data::ID_OPAQUE)),
    ]
}

pub(super) fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

pub(super) fn put_string(out: &mut Vec<u8>, bytes: &[u8]) {
    put_u32(out, bytes.len() as u32);
    out.extend_from_slice(bytes);
}

pub(super) fn public_of(seed: &[u8; 32]) -> [u8; 32] {
    SigningKey::from_bytes(seed).verifying_key().to_bytes()
}

pub(super) fn public_blob(public: &[u8; 32]) -> Vec<u8> {
    let mut blob = Vec::new();
    put_string(&mut blob, b"ssh-ed25519");
    put_string(&mut blob, public);
    blob
}

/// How the padding after the comment is written.
#[derive(Clone)]
pub(super) enum Pad {
    /// `1,2,..,r` with `r` the least value that reaches a multiple of the block.
    Natural,
    /// Exactly `n` bytes `1,2,..,n` (any `n`, ignoring the block).
    Count(usize),
    /// Arbitrary trailing bytes.
    Raw(Vec<u8>),
}

/// An OpenSSH inner private section for `seed`.
pub(super) fn inner_section(seed: &[u8; 32], comment: &[u8], block: usize, pad: Pad) -> Vec<u8> {
    let public = public_of(seed);
    let mut inner = Vec::new();
    put_u32(&mut inner, 0x0102_0304);
    put_u32(&mut inner, 0x0102_0304);
    put_string(&mut inner, b"ssh-ed25519");
    put_string(&mut inner, &public);
    let mut keypair = seed.to_vec();
    keypair.extend_from_slice(&public);
    put_string(&mut inner, &keypair);
    put_string(&mut inner, comment);
    match pad {
        Pad::Natural => {
            let mut next = 1u8;
            while inner.len() % block != 0 {
                inner.push(next);
                next += 1;
            }
        }
        Pad::Count(count) => inner.extend((1..=count).map(|byte| byte as u8)),
        Pad::Raw(raw) => inner.extend_from_slice(&raw),
    }
    inner
}

pub(super) fn bcrypt_options(salt: &[u8], rounds: u32) -> Vec<u8> {
    let mut options = Vec::new();
    put_string(&mut options, salt);
    put_u32(&mut options, rounds);
    options
}

/// Encrypt a block-aligned inner section exactly as `ssh-keygen -a rounds`
/// does: bcrypt-pbkdf (48 bytes) then AES-256-CTR.
pub(super) fn wrap_inner(inner: &[u8], pass: &[u8], salt: &[u8], rounds: u32) -> Vec<u8> {
    assert_eq!(inner.len() % 16, 0);
    let mut okm = [0u8; 48];
    let mut memory = [0u8; 64];
    bcrypt_pbkdf_with_memory(pass, salt, rounds, &mut okm, &mut memory).unwrap();
    let mut out = inner.to_vec();
    Ctr128BE::<Aes256>::new_from_slices(&okm[..32], &okm[32..])
        .unwrap()
        .apply_keystream(&mut out);
    out
}

/// Every field of an OpenSSH container, so a test can break exactly one.
#[derive(Clone)]
pub(super) struct Craft {
    pub cipher: Vec<u8>,
    pub kdf: Vec<u8>,
    pub options: Vec<u8>,
    pub nkeys: u32,
    pub public_blob: Vec<u8>,
    pub private: Vec<u8>,
    pub trailing: Vec<u8>,
}

impl Craft {
    pub(super) fn plain(seed: &[u8; 32]) -> Self {
        Self {
            cipher: b"none".to_vec(),
            kdf: b"none".to_vec(),
            options: Vec::new(),
            nkeys: 1,
            public_blob: public_blob(&public_of(seed)),
            private: inner_section(seed, b"crafted", 8, Pad::Natural),
            trailing: Vec::new(),
        }
    }

    pub(super) fn wrapped(seed: &[u8; 32], pass: &[u8], salt: &[u8], rounds: u32) -> Self {
        let inner = inner_section(seed, b"crafted", 16, Pad::Natural);
        Self {
            cipher: b"aes256-ctr".to_vec(),
            kdf: b"bcrypt".to_vec(),
            options: bcrypt_options(salt, rounds),
            nkeys: 1,
            public_blob: public_blob(&public_of(seed)),
            private: wrap_inner(&inner, pass, salt, rounds),
            trailing: Vec::new(),
        }
    }

    /// A wrapped container whose ciphertext is arbitrary bytes (never
    /// decrypted by the parser tests).
    pub(super) fn wrapped_opaque(salt: &[u8], rounds: u32, private: Vec<u8>) -> Self {
        let mut craft = Self::plain(&RFC8032_SEED_1);
        craft.cipher = b"aes256-ctr".to_vec();
        craft.kdf = b"bcrypt".to_vec();
        craft.options = bcrypt_options(salt, rounds);
        craft.private = private;
        craft
    }

    pub(super) fn bytes(&self) -> Vec<u8> {
        let mut out = b"openssh-key-v1\0".to_vec();
        put_string(&mut out, &self.cipher);
        put_string(&mut out, &self.kdf);
        put_string(&mut out, &self.options);
        put_u32(&mut out, self.nkeys);
        put_string(&mut out, &self.public_blob);
        put_string(&mut out, &self.private);
        out.extend_from_slice(&self.trailing);
        out
    }
}
