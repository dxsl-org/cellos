use core::cell::Cell;
use core::sync::atomic::{AtomicUsize, Ordering};
mod enroll;

use p256::ecdsa::signature::hazmat::PrehashSigner;
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};
use types::kms::{
    KmsCapabilityReadiness, KmsKeyAlgorithm, KmsProviderKind, RelayP256StatusPayload,
    RelayProviderAssessment,
};

use super::{RelayP256Status, RelaySignError};

pub(crate) const FIXTURE_RELAY_GENERATION: u64 = 7;
pub(crate) const FIXTURE_PROFILE_DIGEST: [u8; 32] = [0x42; 32];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FixtureSignatureBehavior {
    Valid,
    HighS,
    InvalidScalar,
    Corrupt,
}

#[derive(Debug, Clone)]
pub(crate) struct FixtureRelayProvider {
    pub(crate) readiness: KmsCapabilityReadiness,
    pub(crate) assessment: RelayProviderAssessment,
    pub(crate) behavior: FixtureSignatureBehavior,
    pub(crate) access_calls: Option<&'static AtomicUsize>,
    pub(crate) key_creations: Option<&'static AtomicUsize>,
    pub(crate) key_destructions: Option<&'static AtomicUsize>,
    pub(crate) destroy_failures: Option<&'static AtomicUsize>,
    /// 0 = legacy development key; N = promoted enrollment key for
    /// generation N (set atomically by `commit_enrollment`).
    pub(crate) active_generation: Cell<u64>,
    pub(crate) authenticated_time_floor: Cell<u64>,
    pub(crate) active_profile_digest: Cell<[u8; 32]>,
}

impl FixtureRelayProvider {
    pub(crate) const fn production() -> Self {
        Self {
            readiness: KmsCapabilityReadiness::Ready,
            assessment: RelayProviderAssessment::ProductionQualified,
            behavior: FixtureSignatureBehavior::Valid,
            access_calls: None,
            key_creations: None,
            key_destructions: None,
            destroy_failures: None,
            active_generation: Cell::new(0),
            active_profile_digest: Cell::new(FIXTURE_PROFILE_DIGEST),
            authenticated_time_floor: Cell::new(1_700_000_000),
        }
    }

    pub(crate) fn status(&self) -> RelayP256Status {
        self.record_access();
        let key = self.active_signing_key();
        let point = VerifyingKey::from(&key).to_encoded_point(false);
        let mut verifying_key_sec1 = [0u8; 65];
        verifying_key_sec1.copy_from_slice(point.as_bytes());
        RelayP256Status {
            metadata: RelayP256StatusPayload {
                algorithm: KmsKeyAlgorithm::RelayP256Sha256,
                readiness: self.readiness,
                provider: KmsProviderKind::TestHooks,
                assessment: self.assessment,
                reserved: 0,
                relay_generation: self.serving_generation(),
                policy_epoch: 11,
                authenticated_time_floor: self.authenticated_time_floor.get(),
                qualification_epoch: 13,
                active_profile_digest: self.active_profile_digest.get(),
                qualification_record_digest: [0x77; 32],
            },
            verifying_key_sec1,
        }
    }

    pub(crate) fn sign(
        &self,
        transcript_hash: [u8; 32],
        relay_generation: u64,
        active_profile_digest: [u8; 32],
        request_id: u64,
    ) -> Result<[u8; 64], RelaySignError> {
        self.record_access();
        if self.readiness != KmsCapabilityReadiness::Ready {
            return Err(RelaySignError::Unavailable);
        }
        if self.assessment != RelayProviderAssessment::ProductionQualified {
            return Err(RelaySignError::QualificationRequired);
        }
        if relay_generation != self.serving_generation() {
            return Err(RelaySignError::GenerationMismatch);
        }
        if active_profile_digest != self.active_profile_digest.get() {
            return Err(RelaySignError::ProfileMismatch);
        }
        if request_id == 0 {
            return Err(RelaySignError::InvalidRequest);
        }
        let digest = provider_tls_digest(transcript_hash);
        let signature: Signature = self
            .active_signing_key()
            .sign_prehash(&digest)
            .map_err(|_| RelaySignError::Failure)?;
        let normalized = signature.normalize_s().unwrap_or(signature);
        let mut raw = [0u8; 64];
        raw.copy_from_slice(normalized.to_bytes().as_slice());
        match self.behavior {
            FixtureSignatureBehavior::Valid => {}
            FixtureSignatureBehavior::HighS => make_high_s(&mut raw),
            FixtureSignatureBehavior::InvalidScalar => raw[32..].fill(0),
            FixtureSignatureBehavior::Corrupt => raw[0] ^= 1,
        }
        Ok(raw)
    }

    fn record_access(&self) {
        if let Some(calls) = self.access_calls {
            calls.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn fixture_key() -> SigningKey {
    SigningKey::from_bytes(&[0x37; 32].into()).expect("fixture scalar")
}

impl FixtureRelayProvider {
    fn serving_generation(&self) -> u64 {
        match self.active_generation.get() {
            0 => FIXTURE_RELAY_GENERATION,
            generation => generation,
        }
    }

    /// The key that currently signs TLS CertificateVerify and backs status.
    pub(crate) fn active_signing_key(&self) -> SigningKey {
        match self.active_generation.get() {
            0 => fixture_key(),
            generation => Self::enrollment_key(generation),
        }
    }

    /// Atomically promote the pending key to the active signer.
    pub(crate) fn commit_enrollment(
        &self,
        pending_generation: u64,
        active_profile_digest: [u8; 32],
    ) -> Result<[u8; 65], crate::storage::RelaySignError> {
        if pending_generation == 0 {
            return Err(crate::storage::RelaySignError::InvalidRequest);
        }
        self.active_generation.set(pending_generation);
        self.active_profile_digest.set(active_profile_digest);
        let key = Self::enrollment_key(pending_generation);
        let point = VerifyingKey::from(&key).to_encoded_point(false);
        let mut sec1 = [0u8; 65];
        sec1.copy_from_slice(point.as_bytes());
        Ok(sec1)
    }
}

fn provider_tls_digest(transcript_hash: [u8; 32]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update([0x20; 64]);
    hash.update(b"TLS 1.3, client CertificateVerify\0");
    hash.update(transcript_hash);
    hash.finalize().into()
}

fn make_high_s(raw: &mut [u8; 64]) {
    const ORDER: [u8; 32] = [
        0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xbc, 0xe6, 0xfa, 0xad, 0xa7, 0x17, 0x9e, 0x84, 0xf3, 0xb9, 0xca, 0xc2, 0xfc, 0x63,
        0x25, 0x51,
    ];
    let mut borrow = 0u16;
    for index in (0..32).rev() {
        let lhs = ORDER[index] as u16;
        let rhs = raw[32 + index] as u16 + borrow;
        raw[32 + index] = lhs.wrapping_sub(rhs) as u8;
        borrow = (lhs < rhs) as u16;
    }
}
