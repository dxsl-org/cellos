//! Clatter DH adapter with an opaque KMS-owned static key.
//!
//! Ephemeral X25519 keys remain local to the broker. Static-key operations use
//! only a KMS handle and binding epoch; the node private scalar is never stored
//! in this representation.

use clatter::bytearray::{ByteArray, SensitiveByteArray};
use clatter::crypto::dh::X25519;
use clatter::error::{DhError, DhResult};
use clatter::traits::{CryptoComponent, Dh, Rng};
use clatter::KeyPair;
use types::kms::{
    AcquireNodeIdentityPayload, BindingEpoch, BrokerBindingPayload, KmsProviderKind,
    NodeIdentityHandle, NodeIdentityState, NodeIdentityStatusPayload,
};

const PRIVATE_REPR_LEN: usize = 77;
const TAG_LOCAL: u8 = 1;
const TAG_OPAQUE: u8 = 2;
const LOCAL_SECRET: core::ops::Range<usize> = 1..33;
const HANDLE: core::ops::Range<usize> = 1..5;
const BINDING_EPOCH: core::ops::Range<usize> = 5..13;
const PUBLIC_KEY: core::ops::Range<usize> = 45..77;

type PrivateRepr = SensitiveByteArray<[u8; PRIVATE_REPR_LEN]>;
type PublicKey = <X25519 as Dh>::PubKey;
type DhOutput = <X25519 as Dh>::Output;

/// X25519 implementation whose static private-key representation is opaque.
#[derive(Clone)]
pub struct KmsBackedX25519;

impl CryptoComponent for KmsBackedX25519 {
    fn name() -> &'static str {
        X25519::name()
    }
}

impl Dh for KmsBackedX25519 {
    type PrivateKey = PrivateRepr;
    type PubKey = PublicKey;
    type Output = DhOutput;

    fn genkey_rng<R: Rng>(rng: &mut R) -> DhResult<KeyPair<PublicKey, PrivateRepr>> {
        let local = X25519::genkey_rng(rng)?;
        let mut encoded = PrivateRepr::new_zero();
        encoded[0] = TAG_LOCAL;
        encoded[LOCAL_SECRET].copy_from_slice(local.secret.as_slice());
        encoded[PUBLIC_KEY].copy_from_slice(&local.public);
        Ok(KeyPair::new(local.public, encoded))
    }

    fn pubkey(key: &PrivateRepr) -> PublicKey {
        if !matches!(key[0], TAG_LOCAL | TAG_OPAQUE) {
            return [0; 32];
        }
        let mut public = [0u8; 32];
        public.copy_from_slice(&key[PUBLIC_KEY]);
        public
    }

    fn dh(key: &PrivateRepr, peer: &PublicKey) -> DhResult<DhOutput> {
        match key[0] {
            TAG_LOCAL => {
                let local = <X25519 as Dh>::PrivateKey::from_slice(&key[LOCAL_SECRET]);
                X25519::dh(&local, peer).and_then(require_nonzero_output)
            }
            TAG_OPAQUE => opaque_static_dh(key, peer),
            _ => Err(DhError::KeyGeneration),
        }
    }
}

/// Public identity plus the KMS authority needed for static DH.
pub struct OpaqueStaticKey {
    handle: NodeIdentityHandle,
    binding_epoch: BindingEpoch,
    public_key: PublicKey,
}

impl OpaqueStaticKey {
    /// Construct a non-exportable static key reference.
    pub fn new(
        handle: NodeIdentityHandle,
        binding_epoch: BindingEpoch,
        public_key: PublicKey,
    ) -> Option<Self> {
        if handle.0 == 0 || binding_epoch.0 == 0 || public_key.iter().all(|byte| *byte == 0) {
            return None;
        }
        Some(Self {
            handle,
            binding_epoch,
            public_key,
        })
    }

    /// Convert metadata into Clatter's keypair container without private bytes.
    pub fn into_keypair(self) -> KeyPair<PublicKey, PrivateRepr> {
        let mut encoded = PrivateRepr::new_zero();
        encoded[0] = TAG_OPAQUE;
        encoded[HANDLE].copy_from_slice(&self.handle.0.to_le_bytes());
        encoded[BINDING_EPOCH].copy_from_slice(&self.binding_epoch.0.to_le_bytes());
        encoded[PUBLIC_KEY].copy_from_slice(&self.public_key);
        KeyPair::new(self.public_key, encoded)
    }
}

/// Validate one KMS acquisition snapshot and build an opaque static key.
///
/// Every readiness, provider, revision, epoch, and public-key field must agree;
/// a mixed or stale snapshot returns `None` and cannot enable remote identity.
pub fn opaque_key_from_kms(
    binding: BrokerBindingPayload,
    status: NodeIdentityStatusPayload,
    acquired: AcquireNodeIdentityPayload,
) -> Option<OpaqueStaticKey> {
    if binding.binding_epoch.0 == 0
        || status.state != NodeIdentityState::Ready
        || status.remote_allowed != 1
        || status.provider == KmsProviderKind::None
        || status.binding_epoch != binding.binding_epoch
        || status.blob_revision == 0
        || acquired.state != NodeIdentityState::Ready
        || acquired.provider != status.provider
        || acquired.binding_epoch != binding.binding_epoch
        || acquired.blob_revision != status.blob_revision
        || acquired.public_key != status.public_key
    {
        return None;
    }
    OpaqueStaticKey::new(acquired.handle, acquired.binding_epoch, acquired.public_key)
}

fn opaque_static_dh(key: &PrivateRepr, peer: &PublicKey) -> DhResult<DhOutput> {
    let handle = NodeIdentityHandle(read_u32(key.as_slice(), HANDLE.start));
    let binding_epoch = BindingEpoch(read_u64(key.as_slice(), BINDING_EPOCH.start));
    #[cfg(target_os = "none")]
    {
        let client = ostd::clients::KmsClient::connect().map_err(|_| DhError::KeyGeneration)?;
        let secret = client
            .noise_static_dh(handle, binding_epoch, peer)
            .map_err(|_| DhError::KeyGeneration)?;
        require_nonzero_output(DhOutput::from_slice(&secret))
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (handle, binding_epoch, peer);
        Err(DhError::KeyGeneration)
    }
}

fn require_nonzero_output(output: DhOutput) -> DhResult<DhOutput> {
    if output.as_slice().iter().all(|byte| *byte == 0) {
        return Err(DhError::KeyGeneration);
    }
    Ok(output)
}

fn read_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("fixed opaque handle"))
}

fn read_u64(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().expect("fixed binding epoch"))
}

#[cfg(test)]
mod tests;
