//! Ed25519 signature VERIFY (no_std, verify-only) for the signed operator policy
//! (roadmap §G.2 P5). Thin wrapper over `ed25519-compact` so the rest of the
//! kernel depends only on `ed25519::verify` — the backend choice is isolated here.
//!
//! Verify-only: the kernel never signs (the fleet root private key lives offline).
//! `ed25519-compact`'s verify enforces canonical encodings (rejects malformed /
//! non-canonical signatures) — adequate for a public-key signature check where no
//! secret is involved.

/// Verify an Ed25519 signature. Returns `false` on any malformed input or
/// mismatch — never panics.
pub fn verify(pubkey: &[u8; 32], msg: &[u8], sig: &[u8; 64]) -> bool {
    use ed25519_compact::{PublicKey, Signature};
    let Ok(pk) = PublicKey::from_slice(pubkey) else {
        return false;
    };
    let Ok(s) = Signature::from_slice(sig) else {
        return false;
    };
    pk.verify(msg, &s).is_ok()
}

/// RFC 8032 §7.1 self-test (TEST 1, empty message) + a tamper-negative.
/// Returns `true` iff the known-good vector verifies AND a flipped signature is
/// rejected. Used as a boot-time spike check; cheap enough to keep for assurance.
pub fn self_test() -> bool {
    const PUBKEY: [u8; 32] = [
        0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64, 0x07,
        0x3a, 0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68, 0xf7, 0x07,
        0x51, 0x1a,
    ];
    const SIG: [u8; 64] = [
        0xe5, 0x56, 0x43, 0x00, 0xc3, 0x60, 0xac, 0x72, 0x90, 0x86, 0xe2, 0xcc, 0x80, 0x6e, 0x82,
        0x8a, 0x84, 0x87, 0x7f, 0x1e, 0xb8, 0xe5, 0xd9, 0x74, 0xd8, 0x73, 0xe0, 0x65, 0x22, 0x49,
        0x01, 0x55, 0x5f, 0xb8, 0x82, 0x15, 0x90, 0xa3, 0x3b, 0xac, 0xc6, 0x1e, 0x39, 0x70, 0x1c,
        0xf9, 0xb4, 0x6b, 0xd2, 0x5b, 0xf5, 0xf0, 0x59, 0x5b, 0xbe, 0x24, 0x65, 0x51, 0x41, 0x43,
        0x8e, 0x7a, 0x10, 0x0b,
    ];
    // Positive: empty-message vector must verify.
    if !verify(&PUBKEY, &[], &SIG) {
        return false;
    }
    // Negative: a single flipped signature byte must be rejected.
    let mut bad = SIG;
    bad[0] ^= 0x01;
    if verify(&PUBKEY, &[], &bad) {
        return false;
    }
    true
}
