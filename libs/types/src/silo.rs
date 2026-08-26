// SPDX-License-Identifier: MPL-2.0
//! Purpose-specific wire contract for the development AArch64 Silo provider.
//!
//! This is not a general signing interface. The accepted operations are the
//! Phase 1 TLS 1.3 client CertificateVerify operation, public status, and the
//! Phase 3 purpose-bound enrollment triple (create/sign-CRI/destroy) over one
//! pending generation key that never leaves the guest.

/// Fixed IPC frame length for KMS-to-Silo messages.
pub const DEVELOPMENT_SILO_FRAME_LEN: usize = 192;
/// Development relay generation exposed by the reference provider.
pub const DEVELOPMENT_RELAY_GENERATION: u64 = 1;
/// Development profile bound into every reference-provider request.
pub const DEVELOPMENT_PROFILE_DIGEST: [u8; 32] = [0x53; 32];

const MAGIC: [u8; 4] = *b"DSLO";
const VERSION: u8 = 1;
const PAYLOAD: usize = 24;

/// Typed, canonical request accepted by the development Silo service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevelopmentSiloRequest {
    /// Return the relay P-256 public key.
    RelayStatus { request_seq: u64 },
    /// Sign exactly one TLS 1.3 client CertificateVerify transcript.
    SignTls13ClientCertificateVerify {
        request_seq: u64,
        transcript_hash: [u8; 32],
        relay_generation: u64,
        active_profile_digest: [u8; 32],
        request_id: u64,
    },
    /// Create the fresh non-exportable key for one pending generation.
    ///
    /// The nonce is fresh admitted entropy per call so a destroyed key can
    /// never be regenerated from generation facts alone.
    CreateEnrollmentKey {
        request_seq: u64,
        pending_generation: u64,
        nonce: [u8; 32],
    },
    /// Reconstruct the canonical CRI independently and sign it raw.
    SignEnrollmentCri {
        request_seq: u64,
        pending_generation: u64,
        hostname_len: u8,
        hostname: [u8; 64],
    },
    /// Explicitly destroy the pending generation key.
    DestroyEnrollmentKey {
        request_seq: u64,
        pending_generation: u64,
    },
    /// Atomically promote the pending key to the active TLS signer and
    /// retire the previous active key inside the guest. The new serving
    /// (generation, policy/profile digest) tuple is bound into promotion so
    /// status and TLS authorization follow the promoted key dynamically.
    PromoteEnrollmentKey {
        request_seq: u64,
        pending_generation: u64,
        active_profile_digest: [u8; 32],
    },
}

/// Bounded failures returned by the development Silo boundary.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevelopmentSiloError {
    /// Kernel-attested caller is not the live KMS instance.
    Unauthorized = 1,
    /// Sequence is zero, stale, or replayed.
    Sequence = 2,
    /// Frame or mailbox bytes are non-canonical.
    Malformed = 3,
    /// Guest artifact or VM is unavailable.
    Unavailable = 4,
    /// Guest reset or execution faulted; the service is fail-closed.
    GuestFault = 5,
    /// Relay generation does not match the development lane.
    GenerationMismatch = 6,
    /// Active relay profile does not match the development lane.
    ProfileMismatch = 7,
    /// No pending enrollment key exists for the requested generation.
    NoEnrollmentKey = 8,
}

impl DevelopmentSiloError {
    fn from_byte(value: u8) -> Option<Self> {
        Some(match value {
            1 => Self::Unauthorized,
            2 => Self::Sequence,
            3 => Self::Malformed,
            4 => Self::Unavailable,
            5 => Self::GuestFault,
            6 => Self::GenerationMismatch,
            7 => Self::ProfileMismatch,
            8 => Self::NoEnrollmentKey,
            _ => return None,
        })
    }
}

/// Typed response emitted by the development Silo service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevelopmentSiloResponse {
    /// Relay public status; private or seed material is never returned.
    RelayStatus {
        request_seq: u64,
        response_seq: u64,
        verifying_key_sec1: [u8; 65],
    },
    /// Fixed-width normalized P-256 signature (`r || s`).
    Tls13ClientCertificateVerify {
        request_seq: u64,
        response_seq: u64,
        signature: [u8; 64],
    },
    /// Public point of the freshly created enrollment key.
    EnrollmentKeyCreated {
        request_seq: u64,
        response_seq: u64,
        verifying_key_sec1: [u8; 65],
    },
    /// Signature over the independently reconstructed canonical CRI.
    EnrollmentCriSigned {
        request_seq: u64,
        response_seq: u64,
        signature: [u8; 64],
    },
    /// Destruction acknowledged; no key material or handle remains.
    EnrollmentKeyDestroyed { request_seq: u64, response_seq: u64 },
    /// Promotion acknowledged; the new active signer's public point follows.
    EnrollmentKeyPromoted {
        request_seq: u64,
        response_seq: u64,
        verifying_key_sec1: [u8; 65],
    },
    /// A typed bounded failure.
    Error {
        request_seq: u64,
        response_seq: u64,
        error: DevelopmentSiloError,
    },
}

fn header(request_seq: u64, response_seq: u64) -> [u8; DEVELOPMENT_SILO_FRAME_LEN] {
    let mut out = [0; DEVELOPMENT_SILO_FRAME_LEN];
    out[..4].copy_from_slice(&MAGIC);
    out[4] = VERSION;
    out[8..16].copy_from_slice(&request_seq.to_le_bytes());
    out[16..24].copy_from_slice(&response_seq.to_le_bytes());
    out
}

fn parse_header(bytes: &[u8]) -> Option<(u8, u8, u64, u64)> {
    (bytes.len() == DEVELOPMENT_SILO_FRAME_LEN
        && bytes[..4] == MAGIC
        && bytes[4] == VERSION
        && bytes[7] == 0)
        .then(|| (bytes[5], bytes[6], word(bytes, 8), word(bytes, 16)))
}
fn word(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(array(bytes, at))
}
fn array<const N: usize>(bytes: &[u8], at: usize) -> [u8; N] {
    bytes[at..at + N].try_into().ok().unwrap()
}
fn zero(bytes: &[u8]) -> bool {
    bytes.iter().all(|byte| *byte == 0)
}

mod wire;

#[cfg(test)]
mod tests;
