//! The RFC 4252 section 7 input and its digest and verification, against vectors that
//! were not produced by this crate.
//!
//! Provenance: K2 CP1 section 4.3 and `reports/k2-cp2-dossier.md` / the K5 test
//! `cells/services/kms/src/tests/ssh/vectors.rs`: public key
//! `b33eaef3...ab62` (the public key of `ssh-key-0.7.0-rc.11/tests/examples/id_ed25519.pub`),
//! session id `00 01 ... 1f`; the digests and the three signatures were computed with
//! OpenSSL over the exact input bytes (no key was generated). The signatures are
//! deterministic ed25519 outputs and are verified here by the linked dalek.
use vissh::userauth::{MAX_INPUT, MAX_USERNAME, digest, signing_input, verify};

fn hex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

fn hex32(s: &str) -> [u8; 32] {
    hex(s).try_into().unwrap()
}

const PUBLIC: &str = "b33eaef37ea2df7caa010defdea34e241f65f1b529a4f43ed14327f5c54aab62";

fn session_id() -> [u8; 32] {
    std::array::from_fn(|i| i as u8)
}

/// The same input assembled with a test-side string encoder.
fn expected(session: &[u8; 32], user: &[u8], public: &[u8; 32]) -> Vec<u8> {
    fn string(out: &mut Vec<u8>, bytes: &[u8]) {
        out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        out.extend_from_slice(bytes);
    }
    let mut blob = Vec::new();
    string(&mut blob, b"ssh-ed25519");
    string(&mut blob, public);
    let mut out = Vec::new();
    string(&mut out, session);
    out.push(50);
    string(&mut out, user);
    string(&mut out, b"ssh-connection");
    string(&mut out, b"publickey");
    out.push(1);
    string(&mut out, b"ssh-ed25519");
    string(&mut out, &blob);
    out
}

#[test]
fn the_k2_vector_is_147_bytes_with_the_published_digest() {
    let public = hex32(PUBLIC);
    let input = signing_input(&session_id(), "user", &public).unwrap();
    assert_eq!(input.len(), 147);
    // Written out: string session_id | 50 | string "user" | "ssh-connection" | "publickey" |
    // TRUE | "ssh-ed25519" | blob (51 bytes inside a string).
    let literal = hex(concat!(
        "00000020000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
        "32",
        "0000000475736572",
        "0000000e7373682d636f6e6e656374696f6e",
        "000000097075626c69636b6579",
        "01",
        "0000000b7373682d65643235353139",
        "00000033",
        "0000000b7373682d65643235353139",
        "00000020b33eaef37ea2df7caa010defdea34e241f65f1b529a4f43ed14327f5c54aab62",
    ));
    assert_eq!(literal.len(), 147);
    assert_eq!(input.as_slice(), &literal[..]);
    assert_eq!(
        digest(&input),
        hex32("fa3246abe7b89d40c24cd8b330fc0eba243f06d4835cbc88181be36bb4e6de65")
    );
}

#[test]
fn the_input_is_143_plus_the_user_length_and_matches_an_independent_encoder() {
    let public = hex32(PUBLIC);
    for n in 1..=MAX_USERNAME {
        let user = "u".repeat(n);
        let input = signing_input(&session_id(), &user, &public).unwrap();
        assert_eq!(input.len(), 143 + n);
        assert_eq!(
            input.as_slice(),
            &expected(&session_id(), user.as_bytes(), &public)[..]
        );
    }
    assert_eq!(MAX_INPUT, 174);
    // A user name KMS would refuse is not built either.
    assert!(signing_input(&session_id(), "", &public).is_none());
    assert!(signing_input(&session_id(), &"u".repeat(MAX_USERNAME + 1), &public).is_none());
}

#[test]
fn the_three_published_signatures_verify_over_the_exact_input() {
    let public = hex32(PUBLIC);
    let vectors: [(&str, usize, &str, &str); 3] = [
        (
            "u",
            144,
            "7559ff7d7bd10978808bab2447a6bc20efea91ae91e51404d24e172316916e93",
            "85be1a68483e3393c8adee3f034af8bb4458decff846d59e43046a655f16ae850d557f5b20e56a00b06140cd60567609e16e9302e3e1db0a10a1c9322503e70e",
        ),
        (
            "user",
            147,
            "fa3246abe7b89d40c24cd8b330fc0eba243f06d4835cbc88181be36bb4e6de65",
            "38e041cad337b43040aeea295dfeb41e3ca1302b1705919a8a2746f263d162fa6e9a6a5c57fdaf71d85f77b77b3ae088efa722c991aef28ef843208084215803",
        ),
        (
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            174,
            "a5eb9ca6afe088ec3bc8c6a1ee82ba7e74794771e44b88f36fd402ad994f8c4c",
            "f4b2f8f238344ef3009b9ba8d5e0359d1cd4715285f88707de575f485a069ee5403298f54bfe656f1f8118f0736db49e868487049b906bd6ccea5f490007cf0e",
        ),
    ];
    for (user, len, sha, signature) in vectors {
        let input = signing_input(&session_id(), user, &public).unwrap();
        assert_eq!(input.len(), len);
        assert_eq!(
            digest(&input),
            hex32(sha),
            "digest for {} bytes",
            user.len()
        );
        let signature: [u8; 64] = hex(signature).try_into().unwrap();
        assert!(
            verify(&public, &input, &signature),
            "signature for {} bytes",
            user.len()
        );
        // Bound to every part: another session, user, key, message or signature fails.
        let mut other_session = session_id();
        other_session[31] ^= 1;
        let other = signing_input(&other_session, user, &public).unwrap();
        assert!(!verify(&public, &other, &signature));
        let other_user = signing_input(&session_id(), "x", &public).unwrap();
        assert!(!verify(&public, &other_user, &signature));
        let mut other_key = public;
        other_key[0] ^= 1;
        assert!(!verify(&other_key, &input, &signature));
        let mut tampered = signature;
        tampered[10] ^= 1;
        assert!(!verify(&public, &input, &tampered));
        assert!(!verify(&public, &input[..input.len() - 1], &signature));
    }
}

#[test]
fn a_public_key_that_is_not_a_curve_point_verifies_nothing() {
    assert!(!verify(&[0x02; 32], b"message", &[0u8; 64]));
}
