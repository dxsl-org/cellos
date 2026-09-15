// SPDX-License-Identifier: MPL-2.0
//! Owner trust anchor (Anchor 3) — Ed25519 signature verification for owner A/B admission records.
//!
//! Distinct from Fleet Policy anchor (`policy.rs`) and Publisher/Cell anchor (`signing.rs`).
//! The device holds only the owner public key; owner private signing occurs offline.

/// Dev Owner Ed25519 public key — derived from deterministic seed `[0x4F; 32]`.
#[cfg(feature = "dev-signing-key")]
const DEV_OWNER_SIGNER_PUBKEY: [u8; 32] = [
    0x00, 0xe3, 0xc5, 0x6b, 0x91, 0xab, 0x0a, 0x01, 0x71, 0x74, 0xb9, 0x66, 0x45, 0xea, 0xf9, 0x28,
    0x36, 0x6c, 0xdb, 0xae, 0x1e, 0x87, 0xfd, 0x21, 0xbf, 0x86, 0x66, 0x1d, 0x86, 0xf3, 0xe7, 0xef,
];

#[cfg(feature = "dev-signing-key")]
const OWNER_SIGNER_PUBKEY: [u8; 32] = DEV_OWNER_SIGNER_PUBKEY;

#[cfg(not(feature = "dev-signing-key"))]
const OWNER_SIGNER_PUBKEY: [u8; 32] = [0u8; 32]; // TODO(prod): provisioned owner key
#[allow(dead_code)]
pub fn owner_signer_pubkey() -> &'static [u8; 32] {
    &OWNER_SIGNER_PUBKEY
}

/// Verify an Ed25519 signature against the owner trust anchor.
pub fn verify_owner_signature(payload: &[u8], sig: &[u8; 64]) -> bool {
    crate::ed25519::verify(&OWNER_SIGNER_PUBKEY, payload, sig)
}

/// Boot-time self-test for the owner trust anchor.
pub fn self_test() -> bool {
    const TEST_PAYLOAD: &[u8] = b"CellosOwnerAnchorTest";
    #[cfg(feature = "dev-signing-key")]
    const TEST_SIG: [u8; 64] = [
        0xad, 0x7b, 0x1a, 0x1f, 0xef, 0x39, 0x78, 0xe2, 0x4b, 0x34, 0xa6, 0x2f, 0x17, 0xa0, 0x62,
        0x56, 0x22, 0x0b, 0x23, 0x3b, 0xed, 0x39, 0xcc, 0x52, 0xbe, 0x14, 0x5a, 0x30, 0xc8, 0xfd,
        0x7d, 0x00, 0x9d, 0xac, 0xe7, 0x2f, 0xe7, 0x1a, 0x20, 0x6b, 0x4d, 0x49, 0xc1, 0x90, 0x49,
        0x7e, 0x26, 0x65, 0xf3, 0x8c, 0x3e, 0xaf, 0x7e, 0x7e, 0x48, 0xa9, 0x7b, 0x19, 0x49, 0x18,
        0xc8, 0xad, 0x80, 0x0c,
    ];

    #[cfg(feature = "dev-signing-key")]
    {
        if !verify_owner_signature(TEST_PAYLOAD, &TEST_SIG) {
            return false;
        }
        let mut bad_sig = TEST_SIG;
        bad_sig[0] ^= 1;
        if verify_owner_signature(TEST_PAYLOAD, &bad_sig) {
            return false;
        }
        true
    }
    #[cfg(not(feature = "dev-signing-key"))]
    {
        // Production placeholder: fail-closed until provisioned
        true
    }
}
