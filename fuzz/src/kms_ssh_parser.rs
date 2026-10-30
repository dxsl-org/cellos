#![no_main]
//! Fuzz the KMS OpenSSH-v1 ed25519 container parser.
//!
//! The target includes the very file the service compiles (`core`-only, no
//! allocation) so the fuzzer exercises production code, not a copy. Properties:
//! the parser never panics or overflows, and `Ok` implies every structural
//! bound of K5 CP1 section 7.1 holds on the input, re-derived here
//! independently of the parser.
//! Run (needs cargo-fuzz): cargo fuzz run kms_ssh_parser -- -max_total_time=300

use libfuzzer_sys::fuzz_target;

#[path = "../../cells/services/kms/src/ssh/parser.rs"]
#[allow(dead_code)]
mod parser;

fn be32(bytes: &[u8], at: usize) -> Option<usize> {
    let raw: [u8; 4] = bytes.get(at..at.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_be_bytes(raw) as usize)
}

fuzz_target!(|data: &[u8]| {
    let Ok(outer) = parser::parse_outer(data) else {
        return;
    };
    // Independent re-check of the allowlist on the raw bytes.
    assert!(data.starts_with(b"openssh-key-v1\0"));
    let mut at = 15usize;
    let cipher_len = be32(data, at).unwrap();
    let cipher = &data[at + 4..at + 4 + cipher_len];
    at += 4 + cipher_len;
    let kdf_len = be32(data, at).unwrap();
    let kdf = &data[at + 4..at + 4 + kdf_len];
    match (cipher, kdf) {
        (b"none", b"none") => {
            assert_eq!(outer.cipher, parser::Cipher::None);
            assert!(outer.salt.is_empty() && outer.rounds == 0);
            assert!(outer.private.len() % 8 == 0 && outer.private.len() >= 136);
        }
        (b"aes256-ctr", b"bcrypt") => {
            assert_eq!(outer.cipher, parser::Cipher::Aes256Ctr);
            assert!((16..=64).contains(&outer.salt.len()));
            assert!((1..=64).contains(&outer.rounds));
            assert!(outer.private.len() % 16 == 0);
            assert!((144..=2048).contains(&outer.private.len()));
        }
        _ => panic!("accepted a cipher/kdf pair outside the allowlist"),
    }
    // The inner parser must also hold up on arbitrary plaintext.
    let mut seed = [0u8; 32];
    let _ = parser::parse_inner(outer.private, outer.cipher, &outer.public, &mut seed);
});
