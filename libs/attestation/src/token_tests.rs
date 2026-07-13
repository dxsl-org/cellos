//! Tests for [`super::token`] — split out of `token.rs` to keep that file under
//! the 200-LOC law.

use super::*;
use p256::ecdsa::signature::hazmat::PrehashSigner;
use p256::ecdsa::SigningKey;

fn test_key() -> SigningKey {
    // Fixed, reproducible dev seed — mirrors kernel/src/signing.rs's pattern
    // of a deterministic test key, never used outside this test module.
    SigningKey::from_bytes(&[0x37u8; 32].into()).expect("valid P-256 scalar")
}

fn test_body() -> AttestBody {
    AttestBody {
        node_id: [0x01u8; 32],
        measurement_aggregate: [0x02u8; 32],
        alias_pubkey: {
            let vk = VerifyingKey::from(&test_key());
            let point = vk.to_encoded_point(false);
            let mut out = [0u8; 65];
            out.copy_from_slice(point.as_bytes());
            out
        },
        nonce: [0x03u8; 16],
    }
}

/// Shared `sign_fn` closure builder — every test signs with `sign_prehash`
/// the same way; factored out so each test states only what differs.
fn encode_signed(key: &SigningKey, body: &AttestBody) -> [u8; TOKEN_LEN] {
    encode(body, |digest| {
        let sig: Signature = key.sign_prehash(digest).expect("sign_prehash");
        let mut raw = [0u8; SIG_LEN];
        raw.copy_from_slice(&sig.to_bytes());
        raw
    })
}

#[test]
fn round_trip_valid_token() {
    let key = test_key();
    let vk = VerifyingKey::from(&key);
    let body = test_body();
    let blob = encode_signed(&key, &body);

    let parsed = parse_and_verify(&blob, &vk).expect("valid token must verify");
    assert_eq!(parsed, body);
}

#[test]
fn tampered_body_is_rejected() {
    let key = test_key();
    let vk = VerifyingKey::from(&key);
    let mut blob = encode_signed(&key, &test_body());

    // Flip one byte inside the body (node_id's first byte).
    blob[HEADER_LEN] ^= 0x01;

    assert_eq!(parse_and_verify(&blob, &vk), Err(AttestError::BadSignature));
}

#[test]
fn wrong_key_is_rejected() {
    let key = test_key();
    let other_key = SigningKey::from_bytes(&[0x99u8; 32].into()).expect("valid P-256 scalar");
    let other_vk = VerifyingKey::from(&other_key);
    let blob = encode_signed(&key, &test_body());

    assert_eq!(parse_and_verify(&blob, &other_vk), Err(AttestError::BadSignature));
}

#[test]
fn truncated_blob_returns_err_not_panic() {
    let key = test_key();
    let vk = VerifyingKey::from(&key);
    let blob = encode_signed(&key, &test_body());

    assert_eq!(parse_and_verify(&blob[..10], &vk), Err(AttestError::BadLength));
    assert_eq!(parse_and_verify(&[], &vk), Err(AttestError::BadLength));
}

#[test]
fn bad_magic_rejected() {
    let key = test_key();
    let vk = VerifyingKey::from(&key);
    let mut blob = encode_signed(&key, &test_body());
    blob[0] = 0xFF;
    assert_eq!(parse_and_verify(&blob, &vk), Err(AttestError::BadMagic));
}

#[test]
fn bad_version_rejected() {
    let key = test_key();
    let vk = VerifyingKey::from(&key);
    let mut blob = encode_signed(&key, &test_body());
    blob[4] = TOKEN_VERSION.wrapping_add(1);
    assert_eq!(parse_and_verify(&blob, &vk), Err(AttestError::BadVersion));
}
