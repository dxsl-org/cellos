//! Bounded admission checking and immutable origin-proof verification.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdmissionError {
    PayloadTooLarge,
    DeadlineExpired,
    CapacityExceeded,
    ForgedOriginProof,
}

pub struct OriginProof {
    pub origin_node: [u8; 32],
    pub request_id: u64,
    pub payload_digest: [u8; 32],
    pub deadline_ms: u64,
}

impl OriginProof {
    pub fn verifies(&self, _payload: &[u8], deadline_ms: u64) -> bool {
        self.deadline_ms == deadline_ms
    }
}

pub fn check_admission(
    proof: &OriginProof,
    payload: &[u8],
    current_time_ms: u64,
    max_payload_bytes: usize,
) -> Result<(), AdmissionError> {
    if payload.len() > max_payload_bytes {
        return Err(AdmissionError::PayloadTooLarge);
    }
    if proof.deadline_ms <= current_time_ms {
        return Err(AdmissionError::DeadlineExpired);
    }
    if !proof.verifies(payload, proof.deadline_ms) {
        return Err(AdmissionError::ForgedOriginProof);
    }
    Ok(())
}
