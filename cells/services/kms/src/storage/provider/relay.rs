use types::kms::{KmsProviderKind, RotateNodeIdentityReason};

use super::super::capability::{RelayP256Status, RelaySignError};
#[cfg(test)]
use super::super::relay_fixture::FixtureRelayProvider;
use super::{
    C2cProvider, JournalRecord, ProviderAssessment, ProviderEpoch, ProviderOpenResult,
};

#[derive(Debug, Clone, Copy)]
pub(crate) enum ProviderSlot {
    Unavailable,
    #[cfg(test)]
    Fixture(FixtureRelayProvider),
}

impl ProviderSlot {
    pub(crate) fn relay_p256_status(&self) -> RelayP256Status {
        match self {
            Self::Unavailable => RelayP256Status::unavailable(),
            #[cfg(test)]
            Self::Fixture(provider) => provider.status(),
        }
    }

    pub(crate) fn sign_tls13_client_certificate_verify(
        &self,
        _transcript_hash: [u8; 32],
        _relay_generation: u64,
        _active_profile_digest: [u8; 32],
        _request_id: u64,
    ) -> Result<[u8; 64], RelaySignError> {
        match self {
            Self::Unavailable => Err(RelaySignError::Unavailable),
            #[cfg(test)]
            Self::Fixture(provider) => provider.sign(
                _transcript_hash,
                _relay_generation,
                _active_profile_digest,
                _request_id,
            ),
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
