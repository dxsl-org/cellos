#![cfg_attr(not(test), allow(dead_code))]

use blake3::Hasher;
use constant_time_eq::constant_time_eq;
use types::kms::KmsProviderKind;

pub(crate) const STORE_DIR: &str = "/srv/cellos/kms";
pub(crate) const SLOT_A_PATH: &str = "/srv/cellos/kms/slot-a.bin";
pub(crate) const SLOT_B_PATH: &str = "/srv/cellos/kms/slot-b.bin";
pub(crate) type JournalKey = [u8; 32];

const MAGIC: [u8; 4] = *b"CKMS";
const VERSION: u8 = 1;
const SEALED_BYTES_LEN: usize = 64;
const AUTH_LEN: usize = 32;
const DIGEST_LEN: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SlotId {
    A = 0,
    B = 1,
}

impl SlotId {
    pub(crate) fn inactive(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JournalRecord {
    pub(crate) slot: SlotId,
    pub(crate) blob_revision: u64,
    pub(crate) policy_epoch: u64,
    pub(crate) provider: KmsProviderKind,
    pub(crate) public_key: [u8; 32],
    pub(crate) payload_len: u16,
    pub(crate) sealed_leaf: [u8; SEALED_BYTES_LEN],
    pub(crate) previous_slot_digest: [u8; DIGEST_LEN],
}

impl JournalRecord {
    pub(crate) const ENCODED_LEN: usize = 186;

    pub(crate) fn placeholder(
        slot: SlotId,
        blob_revision: u64,
        policy_epoch: u64,
        previous_slot_digest: [u8; DIGEST_LEN],
    ) -> Self {
        Self {
            slot,
            blob_revision,
            policy_epoch,
            provider: KmsProviderKind::None,
            public_key: [0; 32],
            payload_len: 0,
            sealed_leaf: [0; SEALED_BYTES_LEN],
            previous_slot_digest,
        }
    }

    pub(crate) fn encode(&self, key: &JournalKey) -> [u8; Self::ENCODED_LEN] {
        let mut out = [0u8; Self::ENCODED_LEN];
        out[..4].copy_from_slice(&MAGIC);
        out[4] = VERSION;
        out[5] = self.slot as u8;
        out[6] = self.provider as u8;
        out[8..16].copy_from_slice(&self.blob_revision.to_le_bytes());
        out[16..24].copy_from_slice(&self.policy_epoch.to_le_bytes());
        out[24..26].copy_from_slice(&self.payload_len.to_le_bytes());
        out[26..58].copy_from_slice(&self.public_key);
        out[58..122].copy_from_slice(&self.sealed_leaf);
        out[122..154].copy_from_slice(&self.previous_slot_digest);
        let auth = authenticator(key, &out[..154]);
        out[154..].copy_from_slice(&auth);
        out
    }

    pub(crate) fn decode(bytes: &[u8], key: &JournalKey, expected_slot: SlotId) -> Option<Self> {
        if bytes.len() != Self::ENCODED_LEN
            || bytes[..4] != MAGIC
            || bytes[4] != VERSION
            || bytes[5] != expected_slot as u8
        {
            return None;
        }
        let auth = authenticator(key, &bytes[..154]);
        if !constant_time_eq(&auth, &bytes[154..]) {
            return None;
        }
        let provider = match bytes[6] {
            x if x == KmsProviderKind::None as u8 => KmsProviderKind::None,
            _ => return None,
        };
        if provider != KmsProviderKind::None {
            return None;
        }
        let payload_len = u16::from_le_bytes([bytes[24], bytes[25]]);
        if payload_len as usize > SEALED_BYTES_LEN {
            return None;
        }
        let mut public_key = [0; 32];
        public_key.copy_from_slice(&bytes[26..58]);
        let mut sealed_leaf = [0; SEALED_BYTES_LEN];
        sealed_leaf.copy_from_slice(&bytes[58..122]);
        let mut previous_slot_digest = [0; DIGEST_LEN];
        previous_slot_digest.copy_from_slice(&bytes[122..154]);
        Some(Self {
            slot: expected_slot,
            blob_revision: u64::from_le_bytes(bytes[8..16].try_into().ok()?),
            policy_epoch: u64::from_le_bytes(bytes[16..24].try_into().ok()?),
            provider,
            public_key,
            payload_len,
            sealed_leaf,
            previous_slot_digest,
        })
    }

    pub(crate) fn digest(&self, key: &JournalKey) -> [u8; DIGEST_LEN] {
        let mut out = [0; DIGEST_LEN];
        out.copy_from_slice(blake3::hash(&self.encode(key)).as_bytes());
        out
    }
}

pub(crate) fn authenticator(key: &JournalKey, body: &[u8]) -> [u8; AUTH_LEN] {
    let mut hasher = Hasher::new_keyed(key);
    hasher.update(body);
    let mut out = [0; AUTH_LEN];
    out.copy_from_slice(hasher.finalize().as_bytes());
    out
}
