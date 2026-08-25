use super::{put_u32, put_u64, read_32, read_u32, read_u64};
use crate::kms::{
    KmsCapabilityReadiness, KmsKeyAlgorithm, KmsProviderKind, RelayProviderAssessment,
};

/// Public status for the independent Relay P-256 provider capability.
///
/// Profile and qualification digests are full SHA-256 values. Readiness does
/// not imply C2C X25519 readiness, and no field authorizes a signing request.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelayP256StatusPayload {
    pub algorithm: KmsKeyAlgorithm,
    pub readiness: KmsCapabilityReadiness,
    pub provider: KmsProviderKind,
    pub assessment: RelayProviderAssessment,
    pub reserved: u32,
    pub relay_generation: u64,
    pub policy_epoch: u64,
    pub authenticated_time_floor: u64,
    pub qualification_epoch: u64,
    pub active_profile_digest: [u8; 32],
    pub qualification_record_digest: [u8; 32],
}

impl RelayP256StatusPayload {
    pub const LEN: usize = 104;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        out[0] = self.algorithm as u8;
        out[1] = self.readiness as u8;
        out[2] = self.provider as u8;
        out[3] = self.assessment as u8;
        put_u32(&mut out, 4, self.reserved);
        put_u64(&mut out, 8, self.relay_generation);
        put_u64(&mut out, 16, self.policy_epoch);
        put_u64(&mut out, 24, self.authenticated_time_floor);
        put_u64(&mut out, 32, self.qualification_epoch);
        out[40..72].copy_from_slice(&self.active_profile_digest);
        out[72..104].copy_from_slice(&self.qualification_record_digest);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::LEN || read_u32(bytes, 4) != 0 {
            return None;
        }
        Some(Self {
            algorithm: KmsKeyAlgorithm::try_from(bytes[0]).ok()?,
            readiness: KmsCapabilityReadiness::try_from(bytes[1]).ok()?,
            provider: KmsProviderKind::try_from(bytes[2]).ok()?,
            assessment: RelayProviderAssessment::try_from(bytes[3]).ok()?,
            reserved: 0,
            relay_generation: read_u64(bytes, 8),
            policy_epoch: read_u64(bytes, 16),
            authenticated_time_floor: read_u64(bytes, 24),
            qualification_epoch: read_u64(bytes, 32),
            active_profile_digest: read_32(bytes, 40),
            qualification_record_digest: read_32(bytes, 72),
        })
    }
}

const _: () = assert!(core::mem::size_of::<RelayP256StatusPayload>() == 104);
