use types::kms::{KmsProviderKind, RotateNodeIdentityReason};

use super::super::capability::{RelayP256Status, RelaySignError};
#[cfg(test)]
use super::super::relay_fixture::FixtureRelayProvider;
use super::{C2cProvider, JournalRecord, ProviderAssessment, ProviderEpoch, ProviderOpenResult};
#[cfg(all(feature = "development-silo-provider", target_arch = "aarch64", target_os = "none"))]
use super::silo::DevelopmentSiloProvider;

#[derive(Debug)]
pub(crate) enum ProviderSlot {
    Unavailable,
    #[cfg(all(feature = "development-silo-provider", target_arch = "aarch64", target_os = "none"))]
    DevelopmentSilo(DevelopmentSiloProvider),
    #[cfg(test)]
    Fixture(FixtureRelayProvider),
}

impl ProviderSlot {
    pub(crate) fn development_runtime() -> Self {
        #[cfg(all(feature = "development-silo-provider", target_arch = "aarch64", target_os = "none"))]
        return Self::DevelopmentSilo(DevelopmentSiloProvider::new());
        #[cfg(not(all(feature = "development-silo-provider", target_arch = "aarch64", target_os = "none")))]
        Self::Unavailable
    }

    pub(crate) fn relay_p256_status(&mut self) -> RelayP256Status {
        match self {
            Self::Unavailable => RelayP256Status::unavailable(),
            #[cfg(all(feature = "development-silo-provider", target_arch = "aarch64", target_os = "none"))]
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
            #[cfg(all(feature = "development-silo-provider", target_arch = "aarch64", target_os = "none"))]
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
}

impl C2cProvider for ProviderSlot {
    fn kind(&self) -> KmsProviderKind { KmsProviderKind::None }
    fn assess(&self) -> ProviderAssessment { ProviderAssessment::unavailable() }
    fn open_or_provision(&self, _record: Option<&JournalRecord>) -> ProviderOpenResult {
        ProviderOpenResult::Unavailable
    }
    fn seal_epoch(&self) -> ProviderEpoch { ProviderEpoch(0) }
    fn rotate(
        &self,
        _reason: RotateNodeIdentityReason,
        _expected_blob_revision: u64,
    ) -> ProviderOpenResult {
        ProviderOpenResult::Unavailable
    }
}
