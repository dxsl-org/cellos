// SPDX-License-Identifier: MPL-2.0
//! Canonical frame codecs for the development Silo contract.

use super::{
    array, header, parse_header, word, zero, DevelopmentSiloError, DevelopmentSiloRequest,
    DevelopmentSiloResponse, DEVELOPMENT_SILO_FRAME_LEN, PAYLOAD,
};

const OP_STATUS: u8 = 1;
const OP_SIGN_TLS: u8 = 2;
const OP_CREATE_ENROLLMENT: u8 = 3;
const OP_SIGN_CRI: u8 = 4;
const OP_DESTROY_ENROLLMENT: u8 = 5;
const OP_PROMOTE_ENROLLMENT: u8 = 6;

impl DevelopmentSiloRequest {
    /// Encode this request into its canonical fixed-size frame.
    pub fn encode(self) -> [u8; DEVELOPMENT_SILO_FRAME_LEN] {
        let mut out = header(self.request_seq(), 0);
        match self {
            Self::RelayStatus { .. } => out[5] = OP_STATUS,
            Self::SignTls13ClientCertificateVerify {
                transcript_hash,
                relay_generation,
                active_profile_digest,
                request_id,
                ..
            } => {
                out[5] = OP_SIGN_TLS;
                out[PAYLOAD..PAYLOAD + 32].copy_from_slice(&transcript_hash);
                out[56..64].copy_from_slice(&relay_generation.to_le_bytes());
                out[64..96].copy_from_slice(&active_profile_digest);
                out[96..104].copy_from_slice(&request_id.to_le_bytes());
            }
            Self::CreateEnrollmentKey {
                pending_generation,
                nonce,
                ..
            } => {
                out[5] = OP_CREATE_ENROLLMENT;
                out[PAYLOAD..PAYLOAD + 8].copy_from_slice(&pending_generation.to_le_bytes());
                out[PAYLOAD + 8..PAYLOAD + 40].copy_from_slice(&nonce);
            }
            Self::SignEnrollmentCri {
                pending_generation,
                hostname_len,
                hostname,
                ..
            } => {
                out[5] = OP_SIGN_CRI;
                out[PAYLOAD..PAYLOAD + 8].copy_from_slice(&pending_generation.to_le_bytes());
                out[PAYLOAD + 8] = hostname_len;
                out[PAYLOAD + 9..PAYLOAD + 73].copy_from_slice(&hostname);
            }
            Self::DestroyEnrollmentKey {
                pending_generation, ..
            } => {
                out[5] = OP_DESTROY_ENROLLMENT;
                out[PAYLOAD..PAYLOAD + 8].copy_from_slice(&pending_generation.to_le_bytes());
            }
            Self::PromoteEnrollmentKey {
                pending_generation,
                active_profile_digest,
                ..
            } => {
                out[5] = OP_PROMOTE_ENROLLMENT;
                out[PAYLOAD..PAYLOAD + 8].copy_from_slice(&pending_generation.to_le_bytes());
                out[PAYLOAD + 8..PAYLOAD + 40].copy_from_slice(&active_profile_digest);
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
            OP_STATUS if zero(&bytes[PAYLOAD..]) => Some(Self::RelayStatus { request_seq }),
            OP_SIGN_TLS if zero(&bytes[104..]) => Some(Self::SignTls13ClientCertificateVerify {
                request_seq,
                transcript_hash: array(bytes, PAYLOAD),
                relay_generation: word(bytes, 56),
                active_profile_digest: array(bytes, 64),
                request_id: word(bytes, 96),
            }),
            OP_CREATE_ENROLLMENT if zero(&bytes[PAYLOAD + 40..]) => {
                Some(Self::CreateEnrollmentKey {
                    request_seq,
                    pending_generation: word(bytes, PAYLOAD),
                    nonce: array(bytes, PAYLOAD + 8),
                })
            }
            OP_SIGN_CRI if zero(&bytes[PAYLOAD + 73..]) => Some(Self::SignEnrollmentCri {
                request_seq,
                pending_generation: word(bytes, PAYLOAD),
                hostname_len: bytes[PAYLOAD + 8],
                hostname: array(bytes, PAYLOAD + 9),
            }),
            OP_DESTROY_ENROLLMENT if zero(&bytes[PAYLOAD + 8..]) => {
                Some(Self::DestroyEnrollmentKey {
                    request_seq,
                    pending_generation: word(bytes, PAYLOAD),
                })
            }
            OP_PROMOTE_ENROLLMENT if zero(&bytes[PAYLOAD + 40..]) => {
                Some(Self::PromoteEnrollmentKey {
                    request_seq,
                    pending_generation: word(bytes, PAYLOAD),
                    active_profile_digest: array(bytes, PAYLOAD + 8),
                })
            }
            _ => None,
        }
    }

    /// Return the nonzero protocol request sequence.
    pub const fn request_seq(self) -> u64 {
        match self {
            Self::RelayStatus { request_seq }
            | Self::SignTls13ClientCertificateVerify { request_seq, .. }
            | Self::CreateEnrollmentKey { request_seq, .. }
            | Self::SignEnrollmentCri { request_seq, .. }
            | Self::DestroyEnrollmentKey { request_seq, .. }
            | Self::PromoteEnrollmentKey { request_seq, .. } => request_seq,
        }
    }

    /// Reject structurally invalid enrollment payloads before any guest call.
    pub fn validate_enrollment(self) -> Option<Self> {
        match self {
            Self::CreateEnrollmentKey {
                pending_generation,
                nonce,
                ..
            } => (pending_generation != 0 && !nonce.iter().all(|byte| *byte == 0)).then_some(self),
            Self::SignEnrollmentCri {
                hostname_len,
                hostname,
                pending_generation,
                ..
            } => {
                let len = hostname_len as usize;
                (pending_generation != 0
                    && len <= 64
                    && hostname[len..].iter().all(|byte| *byte == 0)
                    && len > 0)
                    .then_some(self)
            }
            Self::DestroyEnrollmentKey {
                pending_generation, ..
            } => (pending_generation != 0).then_some(self),
            Self::PromoteEnrollmentKey {
                pending_generation,
                active_profile_digest,
                ..
            } => (pending_generation != 0 && !active_profile_digest.iter().all(|byte| *byte == 0))
                .then_some(self),
            // Only purpose-bound enrollment commands carry payload rules;
            // everything else is structurally canonical already.
            Self::RelayStatus { .. } | Self::SignTls13ClientCertificateVerify { .. } => Some(self),
        }
    }
}

impl DevelopmentSiloResponse {
    /// Encode this response into its canonical fixed-size frame.
    pub fn encode(self) -> [u8; DEVELOPMENT_SILO_FRAME_LEN] {
        let (opcode, request_seq, response_seq, status) = match self {
            Self::RelayStatus {
                request_seq,
                response_seq,
                ..
            } => (OP_STATUS, request_seq, response_seq, 1),
            Self::Tls13ClientCertificateVerify {
                request_seq,
                response_seq,
                ..
            } => (OP_SIGN_TLS, request_seq, response_seq, 1),
            Self::EnrollmentKeyCreated {
                request_seq,
                response_seq,
                ..
            } => (OP_CREATE_ENROLLMENT, request_seq, response_seq, 1),
            Self::EnrollmentCriSigned {
                request_seq,
                response_seq,
                ..
            } => (OP_SIGN_CRI, request_seq, response_seq, 1),
            Self::EnrollmentKeyDestroyed {
                request_seq,
                response_seq,
            } => (OP_DESTROY_ENROLLMENT, request_seq, response_seq, 3),
            Self::EnrollmentKeyPromoted {
                request_seq,
                response_seq,
                ..
            } => (OP_PROMOTE_ENROLLMENT, request_seq, response_seq, 1),
            Self::Error {
                request_seq,
                response_seq,
                error,
            } => (0, request_seq, response_seq, error as u8 + 1),
        };
        let mut out = header(request_seq, response_seq);
        out[5] = opcode;
        out[6] = status;
        match self {
            Self::RelayStatus {
                verifying_key_sec1, ..
            }
            | Self::EnrollmentKeyCreated {
                verifying_key_sec1, ..
            }
            | Self::EnrollmentKeyPromoted {
                verifying_key_sec1, ..
            } => out[PAYLOAD..89].copy_from_slice(&verifying_key_sec1),
            Self::Tls13ClientCertificateVerify { signature, .. }
            | Self::EnrollmentCriSigned { signature, .. } => {
                out[PAYLOAD..88].copy_from_slice(&signature)
            }
            Self::EnrollmentKeyDestroyed { .. } | Self::Error { .. } => {}
        }
        out
    }

    /// Decode a canonical response and reject malformed padding or sequences.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let (opcode, status, request_seq, response_seq) = parse_header(bytes)?;
        if request_seq == 0 || response_seq == 0 || status == 0 {
            return None;
        }
        if opcode == OP_STATUS && status == 1 && zero(&bytes[89..]) {
            return Some(Self::RelayStatus {
                request_seq,
                response_seq,
                verifying_key_sec1: array(bytes, PAYLOAD),
            });
        }
        if opcode == OP_SIGN_TLS && status == 1 && zero(&bytes[88..]) {
            return Some(Self::Tls13ClientCertificateVerify {
                request_seq,
                response_seq,
                signature: array(bytes, PAYLOAD),
            });
        }
        if opcode == OP_CREATE_ENROLLMENT && status == 1 && zero(&bytes[89..]) {
            return Some(Self::EnrollmentKeyCreated {
                request_seq,
                response_seq,
                verifying_key_sec1: array(bytes, PAYLOAD),
            });
        }
        if opcode == OP_SIGN_CRI && status == 1 && zero(&bytes[88..]) {
            return Some(Self::EnrollmentCriSigned {
                request_seq,
                response_seq,
                signature: array(bytes, PAYLOAD),
            });
        }
        if opcode == OP_PROMOTE_ENROLLMENT && status == 1 && zero(&bytes[89..]) {
            return Some(Self::EnrollmentKeyPromoted {
                request_seq,
                response_seq,
                verifying_key_sec1: array(bytes, PAYLOAD),
            });
        }
        if opcode == OP_DESTROY_ENROLLMENT && status == 3 && zero(&bytes[PAYLOAD..]) {
            return Some(Self::EnrollmentKeyDestroyed {
                request_seq,
                response_seq,
            });
        }
        if opcode != 0 {
            return None;
        }
        let error = status
            .checked_sub(1)
            .and_then(DevelopmentSiloError::from_byte)?;
        zero(&bytes[PAYLOAD..]).then_some(Self::Error {
            request_seq,
            response_seq,
            error,
        })
    }
}
