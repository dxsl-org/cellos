//! Silo-backed enrollment key creation, proof signing, and destruction.
//!
//! Methods live in this child module only to keep `silo.rs` small; they are
//! inherent impls on [`DevelopmentSiloProvider`].

use core::hint::black_box;

use ostd::syscall::sys_get_random;
use types::kms::RELAY_HOSTNAME_MAX;
use types::silo::{DevelopmentSiloRequest, DevelopmentSiloResponse};

use super::{map_error, DevelopmentSiloProvider};
use crate::storage::provider::EnrollmentKeyProof;
use crate::storage::{EnrollmentKeyDestroyConfirmation, RelaySignError};

impl DevelopmentSiloProvider {
    /// Create the fresh per-generation key and obtain its raw CRI proof.
    ///
    /// The 32-byte nonce is fresh admitted entropy per call, zeroized on the
    /// KMS side right after the request is encoded and sent.
    pub(crate) fn begin_enrollment(
        &mut self,
        pending_generation: u64,
        hostname: &[u8],
    ) -> Result<EnrollmentKeyProof, RelaySignError> {
        if pending_generation == 0 || hostname.is_empty() || hostname.len() > RELAY_HOSTNAME_MAX {
            return Err(RelaySignError::InvalidRequest);
        }
        let mut nonce = [0u8; 32];
        let mut filled = 0;
        for _ in 0..64 {
            if filled == nonce.len() {
                break;
            }
            let written = sys_get_random(&mut nonce[filled..]);
            if written == 0 {
                break;
            }
            filled += written;
        }
        if filled != nonce.len() {
            nonce.fill(0);
            black_box(&nonce);
            return Err(RelaySignError::Unavailable);
        }
        let request_seq = self.take_request_seq()?;
        let create = DevelopmentSiloRequest::CreateEnrollmentKey {
            request_seq,
            pending_generation,
            nonce,
        };
        nonce.fill(0);
        black_box(&nonce);
        let create_response = match self.call(create) {
            Ok(response) => response,
            Err(error) => {
                if self.destroy_enrollment_key(pending_generation).is_err() {
                    return Err(RelaySignError::CleanupFailed);
                }
                return Err(error);
            }
        };
        let verifying_key_sec1 = match create_response {
            DevelopmentSiloResponse::EnrollmentKeyCreated {
                verifying_key_sec1, ..
            } => verifying_key_sec1,
            DevelopmentSiloResponse::Error { error, .. } => {
                if self.destroy_enrollment_key(pending_generation).is_err() {
                    return Err(RelaySignError::CleanupFailed);
                }
                return Err(map_error(error));
            }
            _ => {
                if self.destroy_enrollment_key(pending_generation).is_err() {
                    return Err(RelaySignError::CleanupFailed);
                }
                return Err(RelaySignError::Failure);
            }
        };
        // Any post-create proof failure must be followed by confirmed cleanup.
        let signature = match self.sign_cri(pending_generation, hostname) {
            Ok(signature) => signature,
            Err(error) => {
                if self.destroy_enrollment_key(pending_generation).is_err() {
                    return Err(RelaySignError::CleanupFailed);
                }
                return Err(error);
            }
        };
        Ok(EnrollmentKeyProof {
            spki_sec1: verifying_key_sec1,
            signature,
        })
    }

    /// Ask the guest to reconstruct the canonical CRI itself and sign it.
    pub(crate) fn sign_cri(
        &mut self,
        pending_generation: u64,
        hostname: &[u8],
    ) -> Result<[u8; 64], RelaySignError> {
        let mut hostname_padded = [0u8; RELAY_HOSTNAME_MAX];
        hostname_padded[..hostname.len()].copy_from_slice(hostname);
        let request_seq = self.take_request_seq()?;
        let request = DevelopmentSiloRequest::SignEnrollmentCri {
            request_seq,
            pending_generation,
            hostname_len: hostname.len() as u8,
            hostname: hostname_padded,
        };
        match self.call(request)? {
            DevelopmentSiloResponse::EnrollmentCriSigned { signature, .. } => Ok(signature),
            DevelopmentSiloResponse::Error { error, .. } => Err(map_error(error)),
            _ => self.fail(RelaySignError::Failure),
        }
    }

    /// Destroy the pending generation key. Deleted and already-absent are
    /// distinct typed confirmations; all transport/protocol failures surface.
    pub(crate) fn destroy_enrollment_key(
        &mut self,
        pending_generation: u64,
    ) -> Result<EnrollmentKeyDestroyConfirmation, RelaySignError> {
        if pending_generation == 0 {
            return Err(RelaySignError::InvalidRequest);
        }
        let request_seq = self.take_cleanup_request_seq()?;
        let request = DevelopmentSiloRequest::DestroyEnrollmentKey {
            request_seq,
            pending_generation,
        };
        match self.call(request)? {
            DevelopmentSiloResponse::EnrollmentKeyDestroyed { .. } => {
                Ok(EnrollmentKeyDestroyConfirmation::Deleted)
            }
            DevelopmentSiloResponse::Error {
                error: types::silo::DevelopmentSiloError::NoEnrollmentKey,
                ..
            } => Ok(EnrollmentKeyDestroyConfirmation::AlreadyAbsent),
            DevelopmentSiloResponse::Error { error, .. } => Err(map_error(error)),
            _ => self.fail(RelaySignError::Failure),
        }
    }
    /// Atomically promote the pending key to the active TLS signer inside
    pub(crate) fn commit_enrollment(
        &mut self,
        pending_generation: u64,
        active_profile_digest: [u8; 32],
    ) -> Result<[u8; 65], RelaySignError> {
        let request_seq = self.take_request_seq()?;
        let request = DevelopmentSiloRequest::PromoteEnrollmentKey {
            request_seq,
            pending_generation,
            active_profile_digest,
        };
        match self.call(request)? {
            DevelopmentSiloResponse::EnrollmentKeyPromoted {
                verifying_key_sec1, ..
            } => {
                self.active_relay_generation = pending_generation;
                self.active_profile_digest = active_profile_digest;
                Ok(verifying_key_sec1)
            }
            DevelopmentSiloResponse::Error { error, .. } => Err(map_error(error)),
            _ => self.fail(RelaySignError::Failure),
        }
    }
}
