//! Host-testable KMS-only protocol state used by the Silo runtime.

use types::silo::{
    DevelopmentSiloError, DevelopmentSiloRequest, DevelopmentSiloResponse,
    DEVELOPMENT_PROFILE_DIGEST, DEVELOPMENT_RELAY_GENERATION,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeerIdentity {
    pub sender_tid: usize,
    pub cell_id: u64,
    pub generation: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct KmsBinding {
    tid: usize,
    cell_id: u64,
    generation: u64,
}

/// Purpose-bound guest surface. Each method maps to exactly one guest
/// command; there is deliberately no generic signing or digest operation.
pub trait PurposeGuest {
    type Error;
    /// Classify a destroy miss separately from execution/transport failure.
    fn classify_destroy_error(error: &Self::Error) -> DevelopmentSiloError;

    fn public_key(&self) -> [u8; 65];
    fn sign_tls13_client_certificate_verify(
        &mut self,
        transcript_hash: [u8; 32],
    ) -> Result<[u8; 64], Self::Error>;
    /// Create the fresh per-enrollment key; `nonce` is fresh admitted entropy.
    fn create_enrollment_key(
        &mut self,
        pending_generation: u64,
        nonce: &[u8; 32],
    ) -> Result<[u8; 65], Self::Error>;
    /// Reconstruct the canonical CRI independently and sign it raw.
    fn sign_enrollment_cri(
        &mut self,
        pending_generation: u64,
        hostname: &[u8],
    ) -> Result<[u8; 64], Self::Error>;
    /// Destroy the pending generation key explicitly.
    fn destroy_enrollment_key(&mut self, pending_generation: u64) -> Result<(), Self::Error>;
    /// Promote the pending key to the active signer for (generation, digest).
    fn promote_enrollment_key(&mut self, pending_generation: u64) -> Result<[u8; 65], Self::Error>;
}

pub struct ProtocolState {
    binding: Option<KmsBinding>,
    last_request_seq: u64,
    next_response_seq: u64,
    guest_available: bool,
    /// Dynamic serving tuple; promotion updates both, TLS requests must
    /// match them instead of compile-time constants.
    active_relay_generation: u64,
    active_profile_digest: [u8; 32],
}

impl ProtocolState {
    pub const fn new() -> Self {
        Self {
            binding: None,
            last_request_seq: 0,
            next_response_seq: 1,
            guest_available: true,
            active_relay_generation: DEVELOPMENT_RELAY_GENERATION,
            active_profile_digest: DEVELOPMENT_PROFILE_DIGEST,
        }
    }

    /// Authorize the live KMS before parsing attacker-controlled request bytes.
    pub fn process<G: PurposeGuest>(
        &mut self,
        guest: &mut G,
        sender: usize,
        live_kms_tid: Option<usize>,
        peer: Option<PeerIdentity>,
        frame: &[u8],
    ) -> Option<DevelopmentSiloResponse> {
        if !self.authorize(sender, live_kms_tid, peer) {
            return Some(error(1, 1, DevelopmentSiloError::Unauthorized));
        }
        let request = match DevelopmentSiloRequest::decode(frame)
            .and_then(DevelopmentSiloRequest::validate_enrollment)
        {
            Some(request) => request,
            None => return self.failure(1, DevelopmentSiloError::Malformed),
        };
        let request_seq = request.request_seq();
        if request_seq <= self.last_request_seq {
            return self.failure(request_seq, DevelopmentSiloError::Sequence);
        }
        self.last_request_seq = request_seq;
        let response_seq = self.take_response_seq()?;
        Some(self.handle(guest, request, response_seq))
    }

    fn authorize(
        &mut self,
        sender: usize,
        live_kms_tid: Option<usize>,
        peer: Option<PeerIdentity>,
    ) -> bool {
        let Some(identity) = peer else { return false };
        if live_kms_tid != Some(sender) || identity.sender_tid != sender || identity.generation == 0
        {
            return false;
        }
        let presented = KmsBinding {
            tid: sender,
            cell_id: identity.cell_id,
            generation: identity.generation,
        };
        if self.binding != Some(presented) {
            self.binding = Some(presented);
            self.last_request_seq = 0;
            self.next_response_seq = 1;
        }
        true
    }

    fn handle<G: PurposeGuest>(
        &mut self,
        guest: &mut G,
        request: DevelopmentSiloRequest,
        response_seq: u64,
    ) -> DevelopmentSiloResponse {
        let request_seq = request.request_seq();
        if !self.guest_available {
            return error(request_seq, response_seq, DevelopmentSiloError::Unavailable);
        }
        match request {
            DevelopmentSiloRequest::RelayStatus { .. } => DevelopmentSiloResponse::RelayStatus {
                request_seq,
                response_seq,
                verifying_key_sec1: guest.public_key(),
            },
            DevelopmentSiloRequest::SignTls13ClientCertificateVerify {
                transcript_hash,
                relay_generation,
                active_profile_digest,
                request_id,
                ..
            } => {
                if relay_generation != self.active_relay_generation {
                    return error(
                        request_seq,
                        response_seq,
                        DevelopmentSiloError::GenerationMismatch,
                    );
                }
                if active_profile_digest != self.active_profile_digest {
                    return error(
                        request_seq,
                        response_seq,
                        DevelopmentSiloError::ProfileMismatch,
                    );
                }
                if request_id == 0 {
                    return error(request_seq, response_seq, DevelopmentSiloError::Malformed);
                }
                match guest.sign_tls13_client_certificate_verify(transcript_hash) {
                    Ok(signature) => DevelopmentSiloResponse::Tls13ClientCertificateVerify {
                        request_seq,
                        response_seq,
                        signature,
                    },
                    Err(_) => {
                        self.guest_available = false;
                        error(request_seq, response_seq, DevelopmentSiloError::GuestFault)
                    }
                }
            }
            DevelopmentSiloRequest::CreateEnrollmentKey {
                pending_generation,
                nonce,
                ..
            } => match guest.create_enrollment_key(pending_generation, &nonce) {
                Ok(verifying_key_sec1) => DevelopmentSiloResponse::EnrollmentKeyCreated {
                    request_seq,
                    response_seq,
                    verifying_key_sec1,
                },
                Err(_) => {
                    self.guest_available = false;
                    error(request_seq, response_seq, DevelopmentSiloError::GuestFault)
                }
            },
            DevelopmentSiloRequest::SignEnrollmentCri {
                pending_generation,
                hostname_len,
                hostname,
                ..
            } => {
                let len = hostname_len as usize;
                if len == 0 || len > 64 {
                    return error(request_seq, response_seq, DevelopmentSiloError::Malformed);
                }
                match guest.sign_enrollment_cri(pending_generation, &hostname[..len]) {
                    Ok(signature) => DevelopmentSiloResponse::EnrollmentCriSigned {
                        request_seq,
                        response_seq,
                        signature,
                    },
                    Err(_) => {
                        self.guest_available = false;
                        error(request_seq, response_seq, DevelopmentSiloError::GuestFault)
                    }
                }
            }
            DevelopmentSiloRequest::PromoteEnrollmentKey {
                pending_generation,
                active_profile_digest,
                ..
            } => {
                match guest.promote_enrollment_key(pending_generation) {
                    Ok(verifying_key_sec1) => {
                        // Promotion is atomic in the guest: adopt the new
                        // tuple only after the guest acknowledged it.
                        self.active_relay_generation = pending_generation;
                        self.active_profile_digest = active_profile_digest;
                        DevelopmentSiloResponse::EnrollmentKeyPromoted {
                            request_seq,
                            response_seq,
                            verifying_key_sec1,
                        }
                    }
                    Err(_) => {
                        // State changes only after guest acknowledgement, so
                        // failed promotion leaves the prior tuple serving.
                        error(
                            request_seq,
                            response_seq,
                            DevelopmentSiloError::NoEnrollmentKey,
                        )
                    }
                }
            }
            DevelopmentSiloRequest::DestroyEnrollmentKey {
                pending_generation, ..
            } => match guest.destroy_enrollment_key(pending_generation) {
                Ok(()) => DevelopmentSiloResponse::EnrollmentKeyDestroyed {
                    request_seq,
                    response_seq,
                },
                Err(guest_error) => {
                    let failure = G::classify_destroy_error(&guest_error);
                    if failure != DevelopmentSiloError::NoEnrollmentKey {
                        self.guest_available = false;
                    }
                    error(request_seq, response_seq, failure)
                }
            },
        }
    }

    fn failure(
        &mut self,
        request_seq: u64,
        failure: DevelopmentSiloError,
    ) -> Option<DevelopmentSiloResponse> {
        self.take_response_seq()
            .map(|response_seq| error(request_seq.max(1), response_seq, failure))
    }

    fn take_response_seq(&mut self) -> Option<u64> {
        let current = self.next_response_seq;
        self.next_response_seq = current.checked_add(1).filter(|seq| *seq != 0).or_else(|| {
            self.guest_available = false;
            None
        })?;
        Some(current)
    }
}

impl Default for ProtocolState {
    fn default() -> Self {
        Self::new()
    }
}

fn error(
    request_seq: u64,
    response_seq: u64,
    failure: DevelopmentSiloError,
) -> DevelopmentSiloResponse {
    DevelopmentSiloResponse::Error {
        request_seq,
        response_seq,
        error: failure,
    }
}

#[cfg(test)]
mod tests;
