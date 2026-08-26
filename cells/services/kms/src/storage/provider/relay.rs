use types::kms::{KmsProviderKind, RotateNodeIdentityReason};

use super::super::capability::{EnrollmentKeyDestroyConfirmation, RelayP256Status, RelaySignError};
#[cfg(test)]
use super::super::relay_fixture::FixtureRelayProvider;
#[cfg(all(
    feature = "development-silo-provider",
    target_arch = "aarch64",
    target_os = "none"
))]
use super::silo::DevelopmentSiloProvider;
use super::{C2cProvider, JournalRecord, ProviderAssessment, ProviderEpoch, ProviderOpenResult};

/// Fresh non-exportable enrollment key material created by the provider.
///
/// `signature` is the raw `r||s` over SHA-256 of the provider's own
/// reconstruction of the canonical CRI for the frozen profile.
pub(crate) struct EnrollmentKeyProof {
    pub spki_sec1: [u8; 65],
    pub signature: [u8; 64],
}

#[derive(Debug)]
pub(crate) enum ProviderSlot {
    Unavailable,
    #[cfg(all(
        feature = "development-silo-provider",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    DevelopmentSilo(DevelopmentSiloProvider),
    #[cfg(test)]
    Fixture(FixtureRelayProvider),
}

impl ProviderSlot {
    pub(crate) fn development_runtime() -> Self {
        #[cfg(all(
            feature = "development-silo-provider",
            target_arch = "aarch64",
            target_os = "none"
        ))]
        return Self::DevelopmentSilo(DevelopmentSiloProvider::new());
        #[cfg(not(all(
            feature = "development-silo-provider",
            target_arch = "aarch64",
            target_os = "none"
        )))]
        Self::Unavailable
    }

    pub(crate) fn relay_p256_status(&mut self) -> RelayP256Status {
        match self {
            Self::Unavailable => RelayP256Status::unavailable(),
            #[cfg(all(
                feature = "development-silo-provider",
                target_arch = "aarch64",
                target_os = "none"
            ))]
            Self::DevelopmentSilo(provider) => provider.status(),
            #[cfg(test)]
            Self::Fixture(provider) => provider.status(),
        }
    }

    pub(crate) fn sign_tls13_client_certificate_verify(
        &mut self,
        transcript_hash: [u8; 32],
        relay_generation: u64,
        active_profile_digest: [u8; 32],
        request_id: u64,
    ) -> Result<[u8; 64], RelaySignError> {
        match self {
            Self::Unavailable => {
                let _ = (
                    transcript_hash,
                    relay_generation,
                    active_profile_digest,
                    request_id,
                );
                Err(RelaySignError::Unavailable)
            }
            #[cfg(all(
                feature = "development-silo-provider",
                target_arch = "aarch64",
                target_os = "none"
            ))]
            Self::DevelopmentSilo(provider) => provider.sign(
                transcript_hash,
                relay_generation,
                active_profile_digest,
                request_id,
            ),
            #[cfg(test)]
            Self::Fixture(provider) => provider.sign(
                transcript_hash,
                relay_generation,
                active_profile_digest,
                request_id,
            ),
        }
    }

    /// Create a fresh non-exportable P-256 key for one pending generation
    /// and prove possession over the provider-reconstructed canonical CRI.
    pub(crate) fn begin_enrollment(
        &mut self,
        pending_generation: u64,
        hostname: &[u8],
    ) -> Result<EnrollmentKeyProof, RelaySignError> {
        match self {
            Self::Unavailable => {
                // Without a provider arm compiled in, these facts would be
                // unused; mirror the sign-path convention of naming them.
                let _ = (pending_generation, hostname);
                Err(RelaySignError::Unavailable)
            }
            #[cfg(all(
                feature = "development-silo-provider",
                target_arch = "aarch64",
                target_os = "none"
            ))]
            Self::DevelopmentSilo(provider) => {
                provider.begin_enrollment(pending_generation, hostname)
            }
            #[cfg(test)]
            Self::Fixture(provider) => provider.begin_enrollment(pending_generation, hostname),
        }
    }

    /// Atomically promote the pending key to the active signer/status key.
    /// Called only between lifecycle prepare_commit and apply_commit.
    pub(crate) fn commit_enrollment(
        &mut self,
        pending_generation: u64,
        active_profile_digest: [u8; 32],
    ) -> Result<[u8; 65], RelaySignError> {
        match self {
            Self::Unavailable => {
                let _ = (pending_generation, active_profile_digest);
                Err(RelaySignError::Unavailable)
            }
            #[cfg(all(
                feature = "development-silo-provider",
                target_arch = "aarch64",
                target_os = "none"
            ))]
            Self::DevelopmentSilo(provider) => {
                provider.commit_enrollment(pending_generation, active_profile_digest)
            }
            #[cfg(test)]
            Self::Fixture(provider) => {
                provider.commit_enrollment(pending_generation, active_profile_digest)
            }
        }
    }

    /// Destroy the pending generation key and return provider confirmation.
    /// Callers retain their cleanup tombstone until this succeeds.
    pub(crate) fn destroy_enrollment_key(
        &mut self,
        pending_generation: u64,
    ) -> Result<EnrollmentKeyDestroyConfirmation, RelaySignError> {
        match self {
            Self::Unavailable => {
                let _ = pending_generation;
                Err(RelaySignError::Unavailable)
            }
            #[cfg(all(
                feature = "development-silo-provider",
                target_arch = "aarch64",
                target_os = "none"
            ))]
            Self::DevelopmentSilo(provider) => provider.destroy_enrollment_key(pending_generation),
            #[cfg(test)]
            Self::Fixture(provider) => provider.destroy_enrollment_key(pending_generation),
        }
    }
}

impl C2cProvider for ProviderSlot {
    fn kind(&self) -> KmsProviderKind {
        KmsProviderKind::None
    }
    fn assess(&self) -> ProviderAssessment {
        ProviderAssessment::unavailable()
    }
    fn open_or_provision(&self, _record: Option<&JournalRecord>) -> ProviderOpenResult {
        ProviderOpenResult::Unavailable
    }
    fn seal_epoch(&self) -> ProviderEpoch {
        ProviderEpoch(0)
    }
    fn rotate(
        &self,
        _reason: RotateNodeIdentityReason,
        _expected_blob_revision: u64,
    ) -> ProviderOpenResult {
        ProviderOpenResult::Unavailable
    }
}
