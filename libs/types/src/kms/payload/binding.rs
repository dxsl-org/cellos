use super::{put_u16, put_u32, put_u64, read_32, read_u16, read_u32, read_u64};
use crate::kms::{BindingEpoch, KmsProviderKind, NodeIdentityHandle, NodeIdentityState};

/// Response to `RegisterBrokerInstance`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrokerBindingPayload {
    pub binding_epoch: BindingEpoch,
    pub bound_cell_id: u64,
    pub bound_generation: u64,
    pub bound_service_tid: u64,
}

impl BrokerBindingPayload {
    pub const LEN: usize = 32;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        put_u64(&mut out, 0, self.binding_epoch.0);
        put_u64(&mut out, 8, self.bound_cell_id);
        put_u64(&mut out, 16, self.bound_generation);
        put_u64(&mut out, 24, self.bound_service_tid);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        (bytes.len() == Self::LEN).then(|| Self {
            binding_epoch: BindingEpoch(read_u64(bytes, 0)),
            bound_cell_id: read_u64(bytes, 8),
            bound_generation: read_u64(bytes, 16),
            bound_service_tid: read_u64(bytes, 24),
        })
    }
}

/// Public readiness response. `remote_allowed` is authoritative only when one.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeIdentityStatusPayload {
    pub state: NodeIdentityState,
    pub provider: KmsProviderKind,
    pub remote_allowed: u8,
    pub reserved: u8,
    pub binding_epoch: BindingEpoch,
    pub blob_revision: u64,
    pub policy_epoch: u64,
    pub public_key: [u8; 32],
}

impl NodeIdentityStatusPayload {
    pub const LEN: usize = 64;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        out[0] = self.state as u8;
        out[1] = self.provider as u8;
        out[2] = self.remote_allowed;
        out[3] = self.reserved;
        put_u64(&mut out, 8, self.binding_epoch.0);
        put_u64(&mut out, 16, self.blob_revision);
        put_u64(&mut out, 24, self.policy_epoch);
        out[32..64].copy_from_slice(&self.public_key);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::LEN || !matches!(bytes[2], 0 | 1) || bytes[3] != 0 {
            return None;
        }
        Some(Self {
            state: NodeIdentityState::try_from(bytes[0]).ok()?,
            provider: KmsProviderKind::try_from(bytes[1]).ok()?,
            remote_allowed: bytes[2],
            reserved: 0,
            binding_epoch: BindingEpoch(read_u64(bytes, 8)),
            blob_revision: read_u64(bytes, 16),
            policy_epoch: read_u64(bytes, 24),
            public_key: read_32(bytes, 32),
        })
    }
}

/// Successful `AcquireNodeIdentity` response.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcquireNodeIdentityPayload {
    pub handle: NodeIdentityHandle,
    pub provider: KmsProviderKind,
    pub state: NodeIdentityState,
    pub reserved: u16,
    pub binding_epoch: BindingEpoch,
    pub blob_revision: u64,
    pub public_key: [u8; 32],
}

impl AcquireNodeIdentityPayload {
    pub const LEN: usize = 56;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        put_u32(&mut out, 0, self.handle.0);
        out[4] = self.provider as u8;
        out[5] = self.state as u8;
        put_u16(&mut out, 6, self.reserved);
        put_u64(&mut out, 8, self.binding_epoch.0);
        put_u64(&mut out, 16, self.blob_revision);
        out[24..56].copy_from_slice(&self.public_key);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::LEN || read_u16(bytes, 6) != 0 {
            return None;
        }
        Some(Self {
            handle: NodeIdentityHandle(read_u32(bytes, 0)),
            provider: KmsProviderKind::try_from(bytes[4]).ok()?,
            state: NodeIdentityState::try_from(bytes[5]).ok()?,
            reserved: 0,
            binding_epoch: BindingEpoch(read_u64(bytes, 8)),
            blob_revision: read_u64(bytes, 16),
            public_key: read_32(bytes, 24),
        })
    }
}

const _: () = assert!(core::mem::size_of::<BrokerBindingPayload>() == 32);
const _: () = assert!(core::mem::size_of::<NodeIdentityStatusPayload>() == 64);
const _: () = assert!(core::mem::size_of::<AcquireNodeIdentityPayload>() == 56);
