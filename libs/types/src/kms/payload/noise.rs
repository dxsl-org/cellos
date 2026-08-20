use super::{put_u16, put_u32, put_u64, read_32, read_u16, read_u32, read_u64};
use crate::kms::{BindingEpoch, NodeIdentityHandle};

/// Broker-only static-DH request. The binding epoch prevents stale handle use.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoiseStaticDhRequestPayload {
    pub handle: NodeIdentityHandle,
    pub key_id: u16,
    pub reserved: u16,
    pub binding_epoch: BindingEpoch,
    pub peer_public_key: [u8; 32],
}

impl NoiseStaticDhRequestPayload {
    pub const LEN: usize = 48;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        put_u32(&mut out, 0, self.handle.0);
        put_u16(&mut out, 4, self.key_id);
        put_u16(&mut out, 6, self.reserved);
        put_u64(&mut out, 8, self.binding_epoch.0);
        out[16..48].copy_from_slice(&self.peer_public_key);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::LEN || read_u16(bytes, 6) != 0 {
            return None;
        }
        Some(Self {
            handle: NodeIdentityHandle(read_u32(bytes, 0)),
            key_id: read_u16(bytes, 4),
            reserved: 0,
            binding_epoch: BindingEpoch(read_u64(bytes, 8)),
            peer_public_key: read_32(bytes, 16),
        })
    }
}

/// Static-DH result. The shared secret is ephemeral; no private scalar exists here.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoiseStaticDhResponsePayload {
    pub handle: NodeIdentityHandle,
    pub reserved: u32,
    pub binding_epoch: BindingEpoch,
    pub shared_secret: [u8; 32],
}

impl NoiseStaticDhResponsePayload {
    pub const LEN: usize = 48;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        put_u32(&mut out, 0, self.handle.0);
        put_u32(&mut out, 4, self.reserved);
        put_u64(&mut out, 8, self.binding_epoch.0);
        out[16..48].copy_from_slice(&self.shared_secret);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::LEN || read_u32(bytes, 4) != 0 {
            return None;
        }
        Some(Self {
            handle: NodeIdentityHandle(read_u32(bytes, 0)),
            reserved: 0,
            binding_epoch: BindingEpoch(read_u64(bytes, 8)),
            shared_secret: read_32(bytes, 16),
        })
    }
}

const _: () = assert!(core::mem::size_of::<NoiseStaticDhRequestPayload>() == 48);
const _: () = assert!(core::mem::size_of::<NoiseStaticDhResponsePayload>() == 48);
