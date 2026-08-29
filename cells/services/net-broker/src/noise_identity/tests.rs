use super::handshake_prologue;
use crate::kms_dh::KmsBackedX25519;
use clatter::{
    crypto::{cipher::ChaChaPoly, hash::Sha256},
    handshakepattern::noise_kk_psk0,
    traits::{Dh, Handshaker},
    KeyPair, NqHandshakeCore,
};
use rand_chacha::ChaCha20Rng;
use rand_core::{CryptoRng, RngCore, SeedableRng};

type Handshake = NqHandshakeCore<KmsBackedX25519, ChaChaPoly, Sha256, TestRng>;

#[derive(Clone)]
struct TestRng(ChaCha20Rng);

impl Default for TestRng {
    fn default() -> Self {
        Self(ChaCha20Rng::from_seed([0x5a; 32]))
    }
}

impl RngCore for TestRng {
    fn next_u32(&mut self) -> u32 {
        self.0.next_u32()
    }

    fn next_u64(&mut self) -> u64 {
        self.0.next_u64()
    }

    fn fill_bytes(&mut self, destination: &mut [u8]) {
        self.0.fill_bytes(destination);
    }

    fn try_fill_bytes(&mut self, destination: &mut [u8]) -> Result<(), rand_core::Error> {
        self.0.try_fill_bytes(destination)
    }
}

impl CryptoRng for TestRng {}

#[test]
fn prologue_layout_is_cluster_then_initiator_then_responder_for_both_roles() {
    let cluster_id = 0x0123_4567_89ab_cdef;
    let initiator_id = core::array::from_fn(|index| 0x80 + index as u8);
    let responder_id = core::array::from_fn(|index| 0x40 + index as u8);
    let initiator_view = handshake_prologue(cluster_id, &initiator_id, &responder_id, true);
    let responder_view = handshake_prologue(cluster_id, &responder_id, &initiator_id, false);

    for prologue in [initiator_view, responder_view] {
        assert_eq!(&prologue[..8], &cluster_id.to_le_bytes());
        assert_eq!(&prologue[8..40], &initiator_id);
        assert_eq!(&prologue[40..72], &responder_id);
    }
}

#[test]
fn initiator_and_responder_complete_the_same_transcript() {
    let mut rng = TestRng::default();
    let initiator_static = KmsBackedX25519::genkey_rng(&mut rng).expect("initiator static key");
    let responder_static = KmsBackedX25519::genkey_rng(&mut rng).expect("responder static key");
    let initiator_static_public = initiator_static.public;
    let responder_static_public = responder_static.public;
    let initiator_ephemeral =
        KmsBackedX25519::genkey_rng(&mut rng).expect("initiator ephemeral key");
    let responder_ephemeral =
        KmsBackedX25519::genkey_rng(&mut rng).expect("responder ephemeral key");
    let initiator_id = [0x11; 32];
    let responder_id = [0x22; 32];
    let cluster_id = 0x4455_6677_8899_aabb;
    let psk = [0x33; 32];

    let mut initiator = new_handshake(
        handshake_prologue(cluster_id, &initiator_id, &responder_id, true),
        true,
        initiator_static,
        initiator_ephemeral,
        responder_static_public,
        &psk,
    );
    let mut responder = new_handshake(
        handshake_prologue(cluster_id, &responder_id, &initiator_id, false),
        false,
        responder_static,
        responder_ephemeral,
        initiator_static_public,
        &psk,
    );
    let mut message = [0u8; 256];

    let first_len = initiator
        .write_message(&[], &mut message)
        .expect("first transcript message");
    responder
        .read_message(&message[..first_len], &mut [])
        .expect("responder accepts initiator transcript");

    let second_len = responder
        .write_message(&[], &mut message)
        .expect("second transcript message");
    initiator
        .read_message(&message[..second_len], &mut [])
        .expect("initiator accepts responder transcript");
}

fn new_handshake(
    prologue: [u8; 72],
    is_initiator: bool,
    static_key: KeyPair<<KmsBackedX25519 as Dh>::PubKey, <KmsBackedX25519 as Dh>::PrivateKey>,
    ephemeral_key: KeyPair<<KmsBackedX25519 as Dh>::PubKey, <KmsBackedX25519 as Dh>::PrivateKey>,
    peer_static_public: <KmsBackedX25519 as Dh>::PubKey,
    psk: &[u8; 32],
) -> Handshake {
    let mut handshake = Handshake::new(
        noise_kk_psk0(),
        &prologue,
        is_initiator,
        Some(static_key),
        Some(ephemeral_key),
        Some(peer_static_public),
        None,
    )
    .expect("handshake state");
    handshake.push_psk(psk);
    handshake
}
