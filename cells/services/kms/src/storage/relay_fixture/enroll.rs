//! Fixture-side non-exportable enrollment keys and CRI reconstruction.
//!
//! The fixture mirrors what a real provider firmware does: derive a fresh
//! P-256 key per pending generation, reconstruct the canonical CRI itself
//! from the frozen profile, and sign only that reconstruction.

use core::sync::atomic::Ordering;

use p256::ecdsa::signature::hazmat::PrehashSigner;
use p256::ecdsa::{Signature, SigningKey};
use sha2::{Digest, Sha256};
use types::kms::canonical_relay_cri;

use super::{FixtureRelayProvider, FixtureSignatureBehavior};
use crate::storage::provider::EnrollmentKeyProof;
use crate::storage::{EnrollmentKeyDestroyConfirmation, RelaySignError};

impl FixtureRelayProvider {
    /// Derive a fresh key, rebuild the CRI independently, sign it raw.
    pub(crate) fn begin_enrollment(
        &self,
        pending_generation: u64,
        hostname: &[u8],
    ) -> Result<EnrollmentKeyProof, RelaySignError> {
        self.record_creation();
        if self.readiness != types::kms::KmsCapabilityReadiness::Ready {
            return Err(RelaySignError::Unavailable);
        }
        let spki_sec1 = self.enrollment_sec1(pending_generation)?;
        let (cri, cri_len) =
            canonical_relay_cri(hostname, &spki_sec1).ok_or(RelaySignError::InvalidRequest)?;
        let digest: [u8; 32] = Sha256::digest(&cri[..cri_len]).into();
        let signature: Signature = Self::enrollment_key(pending_generation)
            .sign_prehash(&digest)
            .map_err(|_| RelaySignError::Failure)?;
        let normalized = signature.normalize_s().unwrap_or(signature);
        let mut signature = [0u8; 64];
        signature.copy_from_slice(normalized.to_bytes().as_slice());
        if self.behavior == FixtureSignatureBehavior::Corrupt {
            signature[0] ^= 1;
        }
        Ok(EnrollmentKeyProof {
            spki_sec1,
            signature,
        })
    }

    /// Destroy the pending generation key and confirm the fixture no longer
    /// retains it. Tests may inject bounded failures to exercise retry.
    pub(crate) fn destroy_enrollment_key(
        &self,
        _pending_generation: u64,
    ) -> Result<EnrollmentKeyDestroyConfirmation, RelaySignError> {
        if let Some(failures) = self.destroy_failures {
            if failures
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_ok()
            {
                return Err(RelaySignError::Failure);
            }
        }
        self.record_destruction();
        Ok(EnrollmentKeyDestroyConfirmation::Deleted)
    }

    fn enrollment_sec1(&self, pending_generation: u64) -> Result<[u8; 65], RelaySignError> {
        let key = Self::enrollment_key(pending_generation);
        let point = p256::ecdsa::VerifyingKey::from(&key).to_encoded_point(false);
        let mut sec1 = [0u8; 65];
        sec1.copy_from_slice(point.as_bytes());
        Ok(sec1)
    }

    pub(crate) fn enrollment_key(pending_generation: u64) -> SigningKey {
        // Deterministic but distinct per generation; hash-retry handles the
        // negligible case of a derived scalar outside the group order.
        let mut material = Sha256::new();
        material.update(b"cellos-relay-enrollment-key-v1");
        material.update(pending_generation.to_le_bytes());
        let mut material: [u8; 32] = material.finalize().into();
        loop {
            if let Ok(key) = SigningKey::from_bytes((&material).into()) {
                return key;
            }
            material = Sha256::digest(material).into();
        }
    }

    pub(crate) fn record_creation(&self) {
        if let Some(calls) = self.key_creations {
            calls.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(crate) fn record_destruction(&self) {
        if let Some(calls) = self.key_destructions {
            calls.fetch_add(1, Ordering::Relaxed);
        }
    }
}
