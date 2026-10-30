//! Known-answer vectors. RFC 8032 pure Ed25519 vectors and the RFC 4252
//! userauth signing inputs, byte for byte.
//!
//! Provenance. RFC 8032 section 7.1 TESTs 1-3 are transcribed from the RFC
//! and equal `ed25519-compact-2.6.0/src/ed25519.rs:1036-1066`; the "SHA(abc)"
//! vector was recalled from the RFC (its text was not available offline) and
//! cross-checked seed -> public -> signature with OpenSSL (python
//! `cryptography`). TEST 1024 is not included: its 1,023-byte message could not
//! be transcribed or cross-checked offline. The userauth signatures were
//! computed with OpenSSL over the public `ssh-key` fixture key
//! (`id_ed25519`, public `b33eaef3...ab62`) and are reproduced here as
//! goldens; no key was generated.

use ed25519_dalek::{Signature, VerifyingKey};

use super::fixtures::*;
use super::*;
use crate::ssh::limits::SIGN_INPUT_MAX;
use crate::ssh::sign::{reconstruct, sign as sign_bytes};

fn check_rfc8032(seed: &str, public: &str, message: &[u8], signature: &str) {
    let seed = hex32(seed);
    let public = hex32(public);
    assert_eq!(public_of(&seed), public, "seed -> public");
    assert_eq!(
        sign_bytes(&seed, message, &public).unwrap().to_vec(),
        hex(signature),
        "signature"
    );
}

#[test]
fn rfc8032_section_7_1_vectors() {
    check_rfc8032(
        "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
        "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
        b"",
        "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
    );
    check_rfc8032(
        "4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb",
        "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
        &[0x72],
        "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
    );
    check_rfc8032(
        "c5aa8df43f9f837bedb7442f31dcb7b166d38535076f094b85ce3a2e0b4458f7",
        "fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025",
        &[0xaf, 0x82],
        "6291d657deec24024827e69c3abe01a30ce548a284743a445e3680d7db5ac3ac18ff9b538d16f290ae67f760984dc6594a7c15e9716ed28dc027beceea1ec40a",
    );
    // SHA(abc): the message is the 64-byte SHA-512 digest of "abc".
    check_rfc8032(
        "833fe62409237b9d62ec77587520911e9a759cec1d19755b7da901b96dca3d42",
        "ec172b93ad5e563bf4932c70e1245034c35467ef2efd4d64ebf819683467e2bf",
        &hex("ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"),
        "dc2a4459e7369633a52b1bf277839a00201009a3efbf3ecb69bea2186c26b58909351fc9ac90b3ecfdfbc7c66431e0303dca179c138ac17ad9bef1177331a704",
    );
}

/// The RFC 4252 section 7 input assembled with the test's own string encoder,
/// independent of `reconstruct`.
fn independent_input(session: &[u8; 32], user: &[u8], public: &[u8; 32]) -> Vec<u8> {
    let mut input = Vec::new();
    put_string(&mut input, session);
    input.push(50);
    put_string(&mut input, user);
    put_string(&mut input, b"ssh-connection");
    put_string(&mut input, b"publickey");
    input.push(1);
    put_string(&mut input, b"ssh-ed25519");
    put_string(&mut input, &public_blob(public));
    input
}

#[test]
fn userauth_input_reconstruction_is_exact() {
    let public = id_ed25519_public();
    for user in [&b"u"[..], b"user", &[b'a'; 31], b"a.b_c-9"] {
        let mut out = [0u8; SIGN_INPUT_MAX];
        let len = reconstruct(&SESSION_ID, user, &public, &mut out);
        assert_eq!(len, 143 + user.len());
        assert_eq!(
            &out[..len],
            independent_input(&SESSION_ID, user, &public).as_slice()
        );
    }
    // The 51-byte public key blob is what `ssh-keygen -lf` fingerprints.
    assert_eq!(
        sha(&public_blob(&public)),
        hex32("5025222ebecf8ecf7014524c0c1c8b81cdcdaed754df8e0e814338e7064f7084")
    );
}

#[test]
fn userauth_signing_vectors_byte_for_byte() {
    let mut service = service();
    let admin = Actor::admin();
    let login = Actor::login();
    import_blob(&mut service, &admin, b"fixture", &id_ed25519()).unwrap();
    let public = id_ed25519_public();
    let vectors: [(&[u8], usize, &str, &str); 3] = [
        (
            b"u",
            144,
            "7559ff7d7bd10978808bab2447a6bc20efea91ae91e51404d24e172316916e93",
            "85be1a68483e3393c8adee3f034af8bb4458decff846d59e43046a655f16ae850d557f5b20e56a00b06140cd60567609e16e9302e3e1db0a10a1c9322503e70e",
        ),
        (
            b"user",
            147,
            "fa3246abe7b89d40c24cd8b330fc0eba243f06d4835cbc88181be36bb4e6de65",
            "38e041cad337b43040aeea295dfeb41e3ca1302b1705919a8a2746f263d162fa6e9a6a5c57fdaf71d85f77b77b3ae088efa722c991aef28ef843208084215803",
        ),
        (
            &[b'a'; 31],
            174,
            "a5eb9ca6afe088ec3bc8c6a1ee82ba7e74794771e44b88f36fd402ad994f8c4c",
            "f4b2f8f238344ef3009b9ba8d5e0359d1cd4715285f88707de575f485a069ee5403298f54bfe656f1f8118f0736db49e868487049b906bd6ccea5f490007cf0e",
        ),
    ];
    for (user, input_len, digest, signature) in vectors {
        let signed = sign(&mut service, &login, public, SESSION_ID, user).unwrap();
        assert_eq!(143 + user.len(), input_len);
        assert_eq!(
            signed.message_sha256,
            hex32(digest),
            "digest for {} bytes",
            user.len()
        );
        assert_eq!(
            signed.signature.to_vec(),
            hex(signature),
            "signature for {} bytes",
            user.len()
        );
        // The reply digest is the digest of the independently assembled input,
        // and the signature verifies over it.
        let input = independent_input(&SESSION_ID, user, &public);
        assert_eq!(sha(&input), signed.message_sha256);
        VerifyingKey::from_bytes(&public)
            .unwrap()
            .verify_strict(&input, &Signature::from_bytes(&signed.signature))
            .unwrap();
    }
}

#[test]
fn signature_bound_to_session_and_user() {
    let mut service = service();
    let admin = Actor::admin();
    let login = Actor::login();
    import_blob(&mut service, &admin, b"fixture", &id_ed25519()).unwrap();
    let public = id_ed25519_public();
    let verifying = VerifyingKey::from_bytes(&public).unwrap();
    let other_session = [0xEEu8; 32];

    let a = sign(&mut service, &login, public, SESSION_ID, b"alice").unwrap();
    let b = sign(&mut service, &login, public, other_session, b"alice").unwrap();
    let c = sign(&mut service, &login, public, SESSION_ID, b"bob").unwrap();
    let input_a = independent_input(&SESSION_ID, b"alice", &public);
    let verifies = |signature: &[u8; 64], input: &[u8]| {
        verifying
            .verify_strict(input, &Signature::from_bytes(signature))
            .is_ok()
    };
    assert!(verifies(&a.signature, &input_a));
    // A signature for session B / user B never verifies against input A.
    assert!(!verifies(&b.signature, &input_a));
    assert!(!verifies(&c.signature, &input_a));
    assert!(verifies(
        &b.signature,
        &independent_input(&other_session, b"alice", &public)
    ));
    assert!(verifies(
        &c.signature,
        &independent_input(&SESSION_ID, b"bob", &public)
    ));
    assert_ne!(a.signature, b.signature);
    assert_ne!(a.signature, c.signature);
    // The signed input never embeds a caller-chosen service, method or algorithm.
    assert_eq!(input_a.len(), 143 + 5);
}
