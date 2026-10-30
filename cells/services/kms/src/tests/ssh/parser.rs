//! Pre-KDF structural allowlist (refusal matrix row 17) and the inner section
//! rules, including the PuTTYgen padding case.

use std::vec;
use std::vec::Vec;

use super::fixtures::*;
use super::*;
use crate::ssh::parser::{parse_inner, parse_outer, Cipher, Reject};
use crate::ssh::unwrap::{plain_seed, KDF_CALLS};

fn kdf_calls() -> usize {
    KDF_CALLS.with(|calls| calls.get())
}

fn outer_reason(blob: &[u8]) -> Result<Cipher, Reject> {
    parse_outer(blob).map(|outer| outer.cipher)
}

#[test]
fn reference_fixtures_pass_the_outer_allowlist() {
    let plain = id_ed25519();
    let outer = parse_outer(&plain).unwrap();
    assert_eq!(outer.cipher, Cipher::None);
    assert_eq!(outer.public, id_ed25519_public());
    assert_eq!(outer.private.len(), 152);
    assert!(outer.salt.is_empty() && outer.rounds == 0);

    let wrapped = id_ed25519_enc();
    let outer = parse_outer(&wrapped).unwrap();
    assert_eq!(outer.cipher, Cipher::Aes256Ctr);
    assert_eq!(outer.public, id_ed25519_public());
    assert_eq!(outer.private.len(), 160);
    assert_eq!(outer.rounds, 16);
    assert_eq!(outer.salt, hex("4a1fdeae8d6ba607afd69d334f8d379a"));
    assert_eq!(kdf_calls(), 0, "the outer parse never runs the KDF");
}

#[test]
fn every_row17_case_rejected_before_kdf() {
    // Published containers of other ciphers and key types.
    for (label, blob) in foreign_fixtures() {
        let mut service = service();
        let admin = Actor::admin();
        let before = kdf_calls();
        assert!(outer_reason(&blob).is_err(), "{label}");
        assert_eq!(
            import_blob(&mut service, &admin, b"k", &blob),
            Err(KmsErrorCode::SshKeyFormatRejected),
            "{label}"
        );
        assert_eq!(kdf_calls(), before, "{label}");
        assert!(service.ssh.identity.is_none(), "{label}");
        assert!(service.ssh.arena.is_zeroed(), "{label}");
    }

    let seed = RFC8032_SEED_1;
    let salt16 = [7u8; 16];
    let mut cases: Vec<(&str, Craft, Reject)> = Vec::new();
    let mut case = |label, craft: Craft, expect| cases.push((label, craft, expect));
    let wrapped = || Craft::wrapped(&seed, PASSPHRASE, &salt16, 2);

    let mut c = Craft::plain(&seed);
    c.cipher = b"aes128-ctr".to_vec();
    case("cipher aes128-ctr", c, Reject::Cipher);
    let mut c = Craft::plain(&seed);
    c.cipher = b"None".to_vec();
    case("cipher case", c, Reject::Cipher);
    let mut c = Craft::plain(&seed);
    c.kdf = b"scrypt".to_vec();
    case("kdf scrypt", c, Reject::Kdf);
    let mut c = Craft::plain(&seed);
    c.kdf = b"bcrypt".to_vec();
    c.options = bcrypt_options(&salt16, 16);
    case("kdf bcrypt with cipher none", c, Reject::CipherKdfMismatch);
    let mut c = wrapped();
    c.kdf = b"none".to_vec();
    c.options = Vec::new();
    case(
        "cipher aes256-ctr with kdf none",
        c,
        Reject::CipherKdfMismatch,
    );
    let mut c = Craft::plain(&seed);
    c.options = vec![0];
    case("kdf none with options", c, Reject::KdfOptions);
    let mut c = wrapped();
    c.options.push(0);
    case("bcrypt options trailing", c, Reject::KdfOptions);
    let mut c = wrapped();
    c.options = bcrypt_options(&salt16, 16)[..10].to_vec();
    case("bcrypt options truncated", c, Reject::Truncated);
    for rounds in [0u32, 65, 0x7fff_ffff, u32::MAX] {
        let mut c = wrapped();
        c.options = bcrypt_options(&salt16, rounds);
        case("rounds out of range", c, Reject::Rounds);
    }
    for salt_len in [0usize, 15, 65, 4096] {
        let mut c = wrapped();
        c.options = bcrypt_options(&vec![9u8; salt_len], 16);
        case("salt out of range", c, Reject::Salt);
    }
    for nkeys in [0u32, 2, u32::MAX] {
        let mut c = Craft::plain(&seed);
        c.nkeys = nkeys;
        case("nkeys", c, Reject::KeyCount);
    }
    let mut c = Craft::plain(&seed);
    c.trailing = vec![0];
    case("trailing byte", c, Reject::Trailing);
    let mut c = Craft::plain(&seed);
    let mut blob = Vec::new();
    put_string(&mut blob, b"ssh-ed25519");
    put_string(&mut blob, &[1u8; 31]);
    c.public_blob = blob;
    case("public key 31 bytes", c, Reject::PublicKey);
    let mut c = Craft::plain(&seed);
    c.public_blob.push(0);
    case("public blob trailing", c, Reject::PublicKey);
    let mut c = Craft::plain(&seed);
    let mut blob = Vec::new();
    put_string(&mut blob, b"ssh-rsa");
    put_string(&mut blob, &[1u8; 32]);
    c.public_blob = blob;
    case("public key type", c, Reject::PublicKey);
    let mut c = Craft::plain(&seed);
    c.private = vec![0; 128];
    case("plain private too short", c, Reject::PrivateLen);
    let mut c = Craft::plain(&seed);
    c.private = vec![0; 140];
    case("plain private not a multiple of 8", c, Reject::PrivateLen);
    case(
        "wrapped private 128",
        Craft::wrapped_opaque(&salt16, 2, vec![0; 128]),
        Reject::PrivateLen,
    );
    case(
        "wrapped private not a multiple of 16",
        Craft::wrapped_opaque(&salt16, 2, vec![0; 152]),
        Reject::PrivateLen,
    );
    case(
        "wrapped private 2064",
        Craft::wrapped_opaque(&salt16, 2, vec![0; 2064]),
        Reject::WrappedTooLarge,
    );
    case(
        "wrapped private 65536",
        Craft::wrapped_opaque(&salt16, 2, vec![0; 65_536]),
        Reject::WrappedTooLarge,
    );

    for (label, craft, expect) in cases {
        let blob = craft.bytes();
        let before = kdf_calls();
        assert_eq!(outer_reason(&blob), Err(expect), "{label}");
        let mut service = service();
        let admin = Actor::admin();
        let code = if blob.len() > 65_536 {
            // Not even transferable: Begin refuses a total above 64 KiB.
            KmsErrorCode::InvalidRequest
        } else if expect == Reject::WrappedTooLarge {
            KmsErrorCode::SshStoreFull
        } else {
            KmsErrorCode::SshKeyFormatRejected
        };
        assert_eq!(
            import_blob(&mut service, &admin, b"k", &blob),
            Err(code),
            "{label}"
        );
        assert_eq!(kdf_calls(), before, "{label}");
        assert!(service.ssh.identity.is_none(), "{label}");
    }
}

#[test]
fn length_fields_at_the_extremes_are_rejected_without_reading_or_allocating() {
    // Overwrite each length-prefixed field of a good container in turn.
    let good = id_ed25519();
    let positions = [15usize, 15 + 4 + 4, 15 + 4 + 4 + 4 + 4];
    for position in positions {
        for value in [0xFFFF_FFFFu32, 0x8000_0000, 0x7FFF_FFFF, 0x0001_0000] {
            let mut blob = good.clone();
            blob[position..position + 4].copy_from_slice(&value.to_be_bytes());
            assert!(parse_outer(&blob).is_err(), "{position} {value:#x}");
        }
    }
    // Every prefix of a good container is refused (no panic, no Ok).
    for len in 0..good.len() {
        assert!(parse_outer(&good[..len]).is_err(), "prefix {len}");
    }
    assert_eq!(kdf_calls(), 0);
}

#[test]
fn boundary_values_that_must_be_accepted_by_the_outer_allowlist() {
    let seed = RFC8032_SEED_1;
    for (salt_len, rounds) in [(16usize, 1u32), (16, 64), (64, 1), (64, 64), (32, 16)] {
        let mut c = Craft::plain(&seed);
        c.cipher = b"aes256-ctr".to_vec();
        c.kdf = b"bcrypt".to_vec();
        c.options = bcrypt_options(&vec![5u8; salt_len], rounds);
        c.private = vec![0; 160];
        let bytes = c.bytes();
        let outer = parse_outer(&bytes).unwrap();
        assert_eq!((outer.salt.len(), outer.rounds), (salt_len, rounds));
    }
    for ct_len in [144usize, 2032, 2048] {
        let c = Craft::wrapped_opaque(&[1u8; 16], 2, vec![0; ct_len]);
        assert_eq!(outer_reason(&c.bytes()), Ok(Cipher::Aes256Ctr), "{ct_len}");
    }
    // Plain: the smallest block-aligned section and a long comment.
    let mut c = Craft::plain(&seed);
    c.private = vec![0; 136];
    assert_eq!(outer_reason(&c.bytes()), Ok(Cipher::None));
    assert_eq!(kdf_calls(), 0);
}

#[test]
fn plain_inner_section_rules() {
    let seed = RFC8032_SEED_1;
    let public = public_of(&seed);

    let ok = |inner: &[u8], cipher| plain_seed(inner, cipher, &public).map(|s| *s);
    // Natural padding to the block, both block sizes.
    assert_eq!(
        ok(&inner_section(&seed, b"c", 8, Pad::Natural), Cipher::None),
        Ok(seed)
    );
    assert_eq!(
        ok(
            &inner_section(&seed, b"c", 16, Pad::Natural),
            Cipher::Aes256Ctr
        ),
        Ok(seed)
    );
    // Padding bounds: none allows up to 16, aes256-ctr up to 15; none past
    // the maximum or with a wrong sequence is refused.
    assert_eq!(
        ok(&inner_section(&seed, b"c", 8, Pad::Count(16)), Cipher::None),
        Ok(seed)
    );
    assert_eq!(
        ok(&inner_section(&seed, b"c", 8, Pad::Count(17)), Cipher::None),
        Err(Reject::Padding)
    );
    assert_eq!(
        ok(
            &inner_section(&seed, b"c", 16, Pad::Count(15)),
            Cipher::Aes256Ctr
        ),
        Ok(seed)
    );
    assert_eq!(
        ok(
            &inner_section(&seed, b"c", 16, Pad::Count(16)),
            Cipher::Aes256Ctr
        ),
        Err(Reject::Padding)
    );
    assert_eq!(
        ok(
            &inner_section(&seed, b"c", 8, Pad::Raw(vec![1, 2, 4])),
            Cipher::None
        ),
        Err(Reject::Padding)
    );
    assert_eq!(
        ok(
            &inner_section(&seed, b"c", 8, Pad::Raw(vec![0])),
            Cipher::None
        ),
        Err(Reject::Padding)
    );
    assert_eq!(
        ok(&inner_section(&seed, b"c", 8, Pad::Count(0)), Cipher::None),
        Ok(seed)
    );

    // Field-level checks.
    let mut section = inner_section(&seed, b"c", 8, Pad::Natural);
    section[7] ^= 1;
    assert_eq!(ok(&section, Cipher::None), Err(Reject::Checkint));
    let mut section = inner_section(&seed, b"c", 8, Pad::Natural);
    section[8 + 4 + 4] = b'x'; // key type
    assert_eq!(ok(&section, Cipher::None), Err(Reject::KeyType));
    let mut section = inner_section(&seed, b"c", 8, Pad::Natural);
    section[8 + 15 + 4] ^= 1; // public key in the section
    assert_eq!(ok(&section, Cipher::None), Err(Reject::PublicMismatch));
    let mut section = inner_section(&seed, b"c", 8, Pad::Natural);
    let public_half = 8 + 15 + 36 + 4 + 32;
    section[public_half] ^= 1; // public half of the keypair
    assert_eq!(ok(&section, Cipher::None), Err(Reject::KeypairLayout));
    // A seed that does not derive the recorded public key (I7).
    let mut section = inner_section(&seed, b"c", 8, Pad::Natural);
    section[8 + 15 + 36 + 4] ^= 1; // first seed byte
    assert_eq!(ok(&section, Cipher::None), Err(Reject::PublicMismatch));
    // A comment whose length runs past the section.
    let mut section = inner_section(&seed, b"c", 8, Pad::Natural);
    let comment_len = 8 + 15 + 36 + 68;
    section[comment_len..comment_len + 4].copy_from_slice(&0xFFFF_FFFFu32.to_be_bytes());
    assert_eq!(ok(&section, Cipher::None), Err(Reject::Truncated));
    // Truncations never panic.
    let section = inner_section(&seed, b"c", 8, Pad::Natural);
    for len in 0..section.len() - 1 {
        let mut out = [0u8; 32];
        let _ = parse_inner(&section[..len], Cipher::None, &public, &mut out);
    }
}

#[test]
fn puttygen_overpadded_key_with_sixteen_padding_bytes_is_accepted() {
    // A real PuTTYgen export: plain ed25519, 176-byte private section with 16
    // padding bytes `1..16` (it uses block size 16 for an unencrypted key).
    let blob = puttygen_overpadded();
    let outer = parse_outer(&blob).unwrap();
    assert_eq!(outer.cipher, Cipher::None);
    assert_eq!(outer.private.len(), 176);
    let seed = plain_seed(outer.private, Cipher::None, &outer.public).unwrap();
    assert_eq!(public_of(&seed), outer.public);

    let mut service = service();
    let admin = Actor::admin();
    let status = import_blob(&mut service, &admin, b"putty", &blob).unwrap();
    assert_eq!(status.custody, KmsSshKeyCustody::Plain);
    assert_eq!(status.public_key, outer.public);
}
