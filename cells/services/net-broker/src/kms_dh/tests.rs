use super::*;
use clatter::crypto::{cipher::ChaChaPoly, hash::Sha256};
use clatter::handshakepattern::noise_kk_psk0;
use clatter::NqHandshakeCore;
use rand_chacha::ChaCha20Rng;
use rand_core::{CryptoRng, RngCore, SeedableRng};

#[derive(Clone)]
struct TestRng(ChaCha20Rng);

impl Default for TestRng {
    fn default() -> Self {
        Self(ChaCha20Rng::from_seed([0x42; 32]))
    }
}

impl RngCore for TestRng {
    fn next_u32(&mut self) -> u32 {
        self.0.next_u32()
    }
    fn next_u64(&mut self) -> u64 {
        self.0.next_u64()
    }
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.0.fill_bytes(dest);
    }
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        self.0.try_fill_bytes(dest)
    }
}

impl CryptoRng for TestRng {}

#[test]
fn opaque_static_key_contains_only_handle_epoch_and_public_metadata() {
    let pair = OpaqueStaticKey::new(NodeIdentityHandle(7), BindingEpoch(9), [0xA5; 32])
        .unwrap()
        .into_keypair();
    assert_eq!(pair.public, [0xA5; 32]);
    assert_eq!(pair.secret[0], TAG_OPAQUE);
    assert_eq!(read_u32(pair.secret.as_slice(), HANDLE.start), 7);
    assert_eq!(read_u64(pair.secret.as_slice(), BINDING_EPOCH.start), 9);
    assert!(pair.secret[13..45].iter().all(|byte| *byte == 0));
    assert_eq!(KmsBackedX25519::pubkey(&pair.secret), pair.public);
}

#[test]
fn clatter_accepts_opaque_static_and_local_ephemeral_keys() {
    let static_pair = OpaqueStaticKey::new(NodeIdentityHandle(7), BindingEpoch(9), [0xA5; 32])
        .unwrap()
        .into_keypair();
    let ephemeral = KmsBackedX25519::genkey_rng(&mut TestRng::default()).unwrap();
    let handshake = NqHandshakeCore::<KmsBackedX25519, ChaChaPoly, Sha256, TestRng>::new(
        noise_kk_psk0(),
        &[],
        true,
        Some(static_pair),
        Some(ephemeral),
        Some([0x5C; 32]),
        None,
    );
    assert!(handshake.is_ok());
}

#[test]
fn opaque_static_dh_is_host_fail_closed() {
    let static_pair = OpaqueStaticKey::new(NodeIdentityHandle(7), BindingEpoch(9), [0xA5; 32])
        .unwrap()
        .into_keypair();
    assert!(matches!(
        KmsBackedX25519::dh(&static_pair.secret, &[0x5C; 32]),
        Err(clatter::error::DhError::KeyGeneration)
    ));
}

#[test]
fn invalid_opaque_metadata_fails_closed() {
    assert!(OpaqueStaticKey::new(NodeIdentityHandle(0), BindingEpoch(1), [1; 32]).is_none());
    assert!(OpaqueStaticKey::new(NodeIdentityHandle(1), BindingEpoch(0), [1; 32]).is_none());
    assert!(OpaqueStaticKey::new(NodeIdentityHandle(1), BindingEpoch(1), [0; 32]).is_none());
    assert_eq!(KmsBackedX25519::pubkey(&PrivateRepr::new_zero()), [0; 32]);
}

#[test]
fn local_ephemeral_dh_rejects_all_zero_output() {
    let local = KmsBackedX25519::genkey_rng(&mut TestRng::default()).unwrap();
    assert!(matches!(
        KmsBackedX25519::dh(&local.secret, &[0; 32]),
        Err(clatter::error::DhError::KeyGeneration)
    ));
}
