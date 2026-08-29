use super::{put_u16, put_u64, read_32, read_u16, read_u32, read_u64};
use crate::kms::RotateNodeIdentityReason;

/// Supervisor-only rotation request with optimistic revision protection.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RotateNodeIdentityRequestPayload {
    pub reason: RotateNodeIdentityReason,
    pub reserved0: u8,
    pub flags: u16,
    pub expected_blob_revision: u64,
}

impl RotateNodeIdentityRequestPayload {
    pub const LEN: usize = 16;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        out[0] = self.reason as u8;
        out[1] = self.reserved0;
        put_u16(&mut out, 2, self.flags);
        put_u64(&mut out, 8, self.expected_blob_revision);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::LEN
            || bytes[1] != 0
            || read_u32(bytes, 4) != 0
            || read_u64(bytes, 8) == 0
        {
            return None;
        }
        Some(Self {
            reason: RotateNodeIdentityReason::try_from(bytes[0]).ok()?,
            reserved0: 0,
            flags: read_u16(bytes, 2),
            expected_blob_revision: read_u64(bytes, 8),
        })
    }
}

/// Rotation result. A new public identity always requires peer re-enrollment.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RotateNodeIdentityResponsePayload {
    pub new_public_key: [u8; 32],
    pub blob_revision: u64,
    pub re_enroll_required: u8,
    pub reserved: [u8; 7],
}

impl RotateNodeIdentityResponsePayload {
    pub const LEN: usize = 48;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        out[..32].copy_from_slice(&self.new_public_key);
        put_u64(&mut out, 32, self.blob_revision);
        out[40] = self.re_enroll_required;
        out[41..48].copy_from_slice(&self.reserved);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::LEN || !matches!(bytes[40], 0 | 1) || bytes[41..48] != [0; 7] {
            return None;
        }
        Some(Self {
            new_public_key: read_32(bytes, 0),
            blob_revision: read_u64(bytes, 32),
            re_enroll_required: bytes[40],
            reserved: [0; 7],
        })
    }
}

const _: () = assert!(core::mem::size_of::<RotateNodeIdentityRequestPayload>() == 16);
const _: () = assert!(core::mem::size_of::<RotateNodeIdentityResponsePayload>() == 48);
