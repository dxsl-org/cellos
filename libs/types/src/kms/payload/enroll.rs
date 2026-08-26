// SPDX-License-Identifier: MPL-2.0
//! Fixed wire payloads for supervisor-only relay certificate enrollment.
//!
//! Opcodes 9-12 carry no certificate chains, private key material, caller
//! supplied CSR bodies, or caller-supplied digests. The CSR itself is built
//! inside KMS from the frozen profile in [`crate::kms::csr`] and leaves KMS
//! only as ordered bounded chunks.

use super::{put_u16, put_u32, put_u64, read_32, read_u16, read_u32, read_u64};
use crate::kms::csr::{validate_hostname, RELAY_CSR_CHUNK_LEN, RELAY_HOSTNAME_MAX};

/// `BeginRelayEnrollment` request: the deployment relay hostname.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelayEnrollmentBeginRequestPayload {
    pub hostname_len: u8,
    pub hostname: [u8; RELAY_HOSTNAME_MAX],
}

impl RelayEnrollmentBeginRequestPayload {
    pub const LEN: usize = 1 + RELAY_HOSTNAME_MAX;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        out[0] = self.hostname_len;
        out[1..].copy_from_slice(&self.hostname);
        out
    }

    /// Decode strictly: length must match the frozen bound, padding beyond
    /// the hostname must be zero, and the hostname must pass the DNS profile.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::LEN || bytes[0] as usize > RELAY_HOSTNAME_MAX {
            return None;
        }
        let len = bytes[0] as usize;
        if bytes[1 + len..].iter().any(|byte| *byte != 0) {
            return None;
        }
        let hostname = &bytes[1..1 + len];
        validate_hostname(hostname).then(|| {
            let mut padded = [0u8; RELAY_HOSTNAME_MAX];
            padded[..len].copy_from_slice(hostname);
            Self {
                hostname_len: bytes[0],
                hostname: padded,
            }
        })
    }

    pub fn hostname(&self) -> &[u8] {
        &self.hostname[..self.hostname_len as usize]
    }
}

/// `BeginRelayEnrollment` response: pending identity plus CSR handle facts.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelayEnrollmentBeginResponsePayload {
    pub pending_relay_generation: u64,
    pub policy_epoch: u64,
    pub restart_epoch: u64,
    pub csr_handle: u64,
    pub csr_len: u32,
    pub reserved: u32,
    pub csr_sha256: [u8; 32],
}

impl RelayEnrollmentBeginResponsePayload {
    pub const LEN: usize = 72;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        put_u64(&mut out, 0, self.pending_relay_generation);
        put_u64(&mut out, 8, self.policy_epoch);
        put_u64(&mut out, 16, self.restart_epoch);
        put_u64(&mut out, 24, self.csr_handle);
        put_u32(&mut out, 32, self.csr_len);
        put_u32(&mut out, 36, self.reserved);
        out[40..72].copy_from_slice(&self.csr_sha256);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::LEN || read_u32(bytes, 36) != 0 {
            return None;
        }
        Some(Self {
            pending_relay_generation: read_u64(bytes, 0),
            policy_epoch: read_u64(bytes, 8),
            restart_epoch: read_u64(bytes, 16),
            csr_handle: read_u64(bytes, 24),
            csr_len: read_u32(bytes, 32),
            reserved: 0,
            csr_sha256: read_32(bytes, 40),
        })
    }
}

/// `ReadRelayCsrChunk` request: one-shot ordered handle plus chunk index.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelayCsrChunkRequestPayload {
    pub csr_handle: u64,
    pub chunk_index: u32,
    pub reserved: u32,
}

impl RelayCsrChunkRequestPayload {
    pub const LEN: usize = 16;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        put_u64(&mut out, 0, self.csr_handle);
        put_u32(&mut out, 8, self.chunk_index);
        put_u32(&mut out, 12, self.reserved);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::LEN || read_u32(bytes, 12) != 0 {
            return None;
        }
        Some(Self {
            csr_handle: read_u64(bytes, 0),
            chunk_index: read_u32(bytes, 8),
            reserved: 0,
        })
    }
}

/// `ReadRelayCsrChunk` response: exactly one ordered CSR slice.
///
/// The 104-byte capacity is [`RELAY_CSR_CHUNK_LEN`]; the final chunk is
/// shorter and zero padded by the frame's canonical tail.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelayCsrChunkResponsePayload {
    pub chunk_index: u32,
    pub chunk_len: u16,
    pub reserved: u16,
    pub chunk: [u8; RELAY_CSR_CHUNK_LEN],
}

impl RelayCsrChunkResponsePayload {
    pub const LEN: usize = 112;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        put_u32(&mut out, 0, self.chunk_index);
        put_u16(&mut out, 4, self.chunk_len);
        put_u16(&mut out, 6, self.reserved);
        out[8..].copy_from_slice(&self.chunk);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::LEN || read_u16(bytes, 6) != 0 {
            return None;
        }
        let chunk_len = read_u16(bytes, 4) as usize;
        if chunk_len == 0 || chunk_len > RELAY_CSR_CHUNK_LEN {
            return None;
        }
        Some(Self {
            chunk_index: read_u32(bytes, 0),
            chunk_len: read_u16(bytes, 4),
            reserved: 0,
            chunk: bytes[8..].try_into().expect("fixed chunk body"),
        })
    }
}

/// `CommitRelayGeneration` request: activate the pending generation under the
/// profile digest the supervisor received from service-net staging.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelayGenerationCommitRequestPayload {
    pub pending_relay_generation: u64,
    pub expected_policy_epoch: u64,
    pub profile_digest: [u8; 32],
}

impl RelayGenerationCommitRequestPayload {
    pub const LEN: usize = 48;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        put_u64(&mut out, 0, self.pending_relay_generation);
        put_u64(&mut out, 8, self.expected_policy_epoch);
        out[16..48].copy_from_slice(&self.profile_digest);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::LEN {
            return None;
        }
        Some(Self {
            pending_relay_generation: read_u64(bytes, 0),
            expected_policy_epoch: read_u64(bytes, 8),
            profile_digest: read_32(bytes, 16),
        })
    }
}

/// `CommitRelayGeneration` response: the now-active protected metadata.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelayGenerationCommitResponsePayload {
    pub active_relay_generation: u64,
    pub policy_epoch: u64,
    pub active_profile_digest: [u8; 32],
}

impl RelayGenerationCommitResponsePayload {
    pub const LEN: usize = 48;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        put_u64(&mut out, 0, self.active_relay_generation);
        put_u64(&mut out, 8, self.policy_epoch);
        out[16..48].copy_from_slice(&self.active_profile_digest);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::LEN {
            return None;
        }
        Some(Self {
            active_relay_generation: read_u64(bytes, 0),
            policy_epoch: read_u64(bytes, 8),
            active_profile_digest: read_32(bytes, 16),
        })
    }
}

/// `AbortRelayEnrollment` request: destroy the named pending generation.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelayEnrollmentAbortRequestPayload {
    pub pending_relay_generation: u64,
}

impl RelayEnrollmentAbortRequestPayload {
    pub const LEN: usize = 8;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        put_u64(&mut out, 0, self.pending_relay_generation);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::LEN {
            return None;
        }
        Some(Self {
            pending_relay_generation: read_u64(bytes, 0),
        })
    }
}

const _: () = assert!(core::mem::size_of::<RelayEnrollmentBeginRequestPayload>() == 65);
const _: () = assert!(core::mem::size_of::<RelayEnrollmentBeginResponsePayload>() == 72);
const _: () = assert!(core::mem::size_of::<RelayCsrChunkRequestPayload>() == 16);
const _: () = assert!(
    core::mem::size_of::<RelayCsrChunkResponsePayload>()
        == 8 + crate::kms::csr::RELAY_CSR_CHUNK_LEN
);
const _: () = assert!(core::mem::size_of::<RelayGenerationCommitRequestPayload>() == 48);
const _: () = assert!(core::mem::size_of::<RelayGenerationCommitResponsePayload>() == 48);
const _: () = assert!(core::mem::size_of::<RelayEnrollmentAbortRequestPayload>() == 8);

/// `StageRelayProfile` request: service-net binds its validated profile
/// digest to the pending generation before any commit can succeed.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelayStageProfileRequestPayload {
    pub pending_relay_generation: u64,
    pub expected_policy_epoch: u64,
    pub profile_digest: [u8; 32],
}

impl RelayStageProfileRequestPayload {
    pub const LEN: usize = 48;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        put_u64(&mut out, 0, self.pending_relay_generation);
        put_u64(&mut out, 8, self.expected_policy_epoch);
        out[16..48].copy_from_slice(&self.profile_digest);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::LEN {
            return None;
        }
        Some(Self {
            pending_relay_generation: read_u64(bytes, 0),
            expected_policy_epoch: read_u64(bytes, 8),
            profile_digest: read_32(bytes, 16),
        })
    }
}

/// `GetRelayActivePublicKey` response: the serving generation's SEC1 point
/// plus `SHA-256(SPKI DER)`. Never carries pending or private material.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelayActivePublicKeyPayload {
    pub relay_generation: u64,
    pub spki_sec1: [u8; 65],
    pub spki_sha256: [u8; 32],
}

impl RelayActivePublicKeyPayload {
    pub const LEN: usize = 105;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        put_u64(&mut out, 0, self.relay_generation);
        out[8..73].copy_from_slice(&self.spki_sec1);
        out[73..105].copy_from_slice(&self.spki_sha256);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::LEN {
            return None;
        }
        Some(Self {
            relay_generation: read_u64(bytes, 0),
            spki_sec1: bytes[8..73].try_into().expect("fixed sec1"),
            spki_sha256: read_32(bytes, 73),
        })
    }
}

const _: () = assert!(core::mem::size_of::<RelayStageProfileRequestPayload>() == 48);
const _: () = assert!(RelayActivePublicKeyPayload::LEN == 105);
