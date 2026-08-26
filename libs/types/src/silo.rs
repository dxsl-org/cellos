// SPDX-License-Identifier: MPL-2.0
//! Purpose-specific wire contract for the development AArch64 Silo provider.
//!
//! This is not a general signing interface. The only accepted operation is the
//! Phase 1 TLS 1.3 client CertificateVerify operation, plus public status.

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
}

/// Bounded failures returned by the development Silo boundary.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevelopmentSiloError {
    /// Kernel-attested caller is not the live KMS instance.
    Unauthorized = 1,
    /// Request or response sequence is zero, stale, or replayed.
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
    /// A typed bounded failure.
    Error {
        request_seq: u64,
        response_seq: u64,
        error: DevelopmentSiloError,
    },
}

impl DevelopmentSiloRequest {
    /// Encode this request into its canonical fixed-size frame.
    pub fn encode(self) -> [u8; DEVELOPMENT_SILO_FRAME_LEN] {
        let mut out = header(self.request_seq(), 0);
        match self {
            Self::RelayStatus { .. } => out[5] = 1,
            Self::SignTls13ClientCertificateVerify {
                transcript_hash,
                relay_generation,
                active_profile_digest,
                request_id,
                ..
            } => {
                out[5] = 2;
                out[PAYLOAD..PAYLOAD + 32].copy_from_slice(&transcript_hash);
                out[56..64].copy_from_slice(&relay_generation.to_le_bytes());
                out[64..96].copy_from_slice(&active_profile_digest);
                out[96..104].copy_from_slice(&request_id.to_le_bytes());
            }
        }
        out
    }

    /// Decode a canonical request, rejecting padding, unknown operations, and zero sequences.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let (opcode, status, request_seq, response_seq) = parse_header(bytes)?;
        if status != 0 || request_seq == 0 || response_seq != 0 {
            return None;
        }
        match opcode {
            1 if zero(&bytes[PAYLOAD..]) => Some(Self::RelayStatus { request_seq }),
            2 if zero(&bytes[104..]) => Some(Self::SignTls13ClientCertificateVerify {
                request_seq,
                transcript_hash: array(bytes, PAYLOAD),
                relay_generation: word(bytes, 56),
                active_profile_digest: array(bytes, 64),
                request_id: word(bytes, 96),
            }),
            _ => None,
        }
    }

    /// Return the nonzero protocol request sequence.
    pub const fn request_seq(self) -> u64 {
        match self {
            Self::RelayStatus { request_seq } | Self::SignTls13ClientCertificateVerify { request_seq, .. } => request_seq,
        }
    }
}

impl DevelopmentSiloResponse {
    /// Encode this response into its canonical fixed-size frame.
    pub fn encode(self) -> [u8; DEVELOPMENT_SILO_FRAME_LEN] {
        let (opcode, request_seq, response_seq, status) = match self {
            Self::RelayStatus { request_seq, response_seq, .. } => (1, request_seq, response_seq, 1),
            Self::Tls13ClientCertificateVerify { request_seq, response_seq, .. } => (2, request_seq, response_seq, 1),
            Self::Error { request_seq, response_seq, error } => (0, request_seq, response_seq, error as u8 + 1),
        };
        let mut out = header(request_seq, response_seq);
        out[5] = opcode;
        out[6] = status;
        match self {
            Self::RelayStatus { verifying_key_sec1, .. } => out[PAYLOAD..89].copy_from_slice(&verifying_key_sec1),
            Self::Tls13ClientCertificateVerify { signature, .. } => out[PAYLOAD..88].copy_from_slice(&signature),
            Self::Error { .. } => {}
        }
        out
    }

    /// Decode a canonical response and reject malformed padding or sequences.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let (opcode, status, request_seq, response_seq) = parse_header(bytes)?;
        if request_seq == 0 || response_seq == 0 || status == 0 {
            return None;
        }
        if opcode == 1 && status == 1 && zero(&bytes[89..]) {
            return Some(Self::RelayStatus { request_seq, response_seq, verifying_key_sec1: array(bytes, PAYLOAD) });
        }
        if opcode == 2 && status == 1 && zero(&bytes[88..]) {
            return Some(Self::Tls13ClientCertificateVerify { request_seq, response_seq, signature: array(bytes, PAYLOAD) });
        }
        if opcode != 0 {
            return None;
        }
        let error = status.checked_sub(1).and_then(DevelopmentSiloError::from_byte)?;
        zero(&bytes[PAYLOAD..]).then_some(Self::Error { request_seq, response_seq, error })
    }
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
    (bytes.len() == DEVELOPMENT_SILO_FRAME_LEN && bytes[..4] == MAGIC && bytes[4] == VERSION && bytes[7] == 0)
        .then(|| (bytes[5], bytes[6], word(bytes, 8), word(bytes, 16)))
}
fn word(bytes: &[u8], at: usize) -> u64 { u64::from_le_bytes(array(bytes, at)) }
fn array<const N: usize>(bytes: &[u8], at: usize) -> [u8; N] { bytes[at..at + N].try_into().ok().unwrap() }
fn zero(bytes: &[u8]) -> bool { bytes.iter().all(|byte| *byte == 0) }

#[cfg(test)]
mod tests;
