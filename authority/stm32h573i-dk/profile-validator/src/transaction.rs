use authority_protocol::{
    verify_root_profile, AuthorityFault, AuthorityState, ProtectedStore, RelayIntent,
    ValidateAndStageRelayProfileRequest, ValidatedRequest,
};
use stm32_authority_journal::PendingEnrollmentSnapshot;

use crate::{adapter::RootProfilePolicy, PendingPublicReader, TrustedPolicy};

/// Supplies a freshly authenticated bank-gated snapshot and its current journal revision.
pub trait PendingSnapshotSource {
    type Error;

    /// Recover and authenticate the current journal plus profile bank after request admission.
    fn load_pending_snapshot(&mut self) -> Result<PendingEnrollmentSnapshot, Self::Error>;

    /// Re-read the authenticated journal head immediately before protected staging.
    fn current_journal_revision(&mut self) -> Result<u64, Self::Error>;
}

/// Fail-closed result from the exclusive validation-and-stage transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileStageError<E> {
    /// Protected request admission, root verification, or staging failed.
    Authority(AuthorityFault),
    /// Authenticated journal or profile-bank recovery failed.
    Snapshot(E),
    /// The journal head changed after validation and before staging.
    JournalChanged,
}

/// Admit, recover authenticated media, validate, recheck revision, and stage one profile.
///
/// The caller must hold the firmware's exclusive enrollment transaction for this entire call.
/// No profile-bank or protected-state operation may interleave between snapshot recovery and
/// `stage_profile`.
///
/// # Errors
///
/// Returns [`ProfileStageError::Authority`] for protected-state or validation rejection,
/// [`ProfileStageError::Snapshot`] for authenticated media/revision read failure, or
/// [`ProfileStageError::JournalChanged`] when the immediate pre-stage revision differs.
pub fn validate_and_stage_profile<S, R, P>(
    state: &mut AuthorityState<S>,
    request: &ValidatedRequest<ValidateAndStageRelayProfileRequest>,
    profile: &[u8],
    policy: TrustedPolicy<'_>,
    snapshots: &mut P,
    public_reader: &mut R,
) -> Result<RelayIntent, ProfileStageError<P::Error>>
where
    S: ProtectedStore,
    R: PendingPublicReader,
    P: PendingSnapshotSource,
{
    let prior_relay = state.relay_state();
    let admitted = state
        .admit_profile_validation(request)
        .map_err(ProfileStageError::Authority)?;
    if let authority_protocol::RelayProfileState::Staged(intent) = prior_relay {
        return Ok(intent);
    }
    let pending = snapshots
        .load_pending_snapshot()
        .map_err(ProfileStageError::Snapshot)?;
    let validated = RootProfilePolicy::new(profile, policy, &pending, public_reader);
    let verified =
        verify_root_profile(admitted, &validated).map_err(ProfileStageError::Authority)?;
    let current_revision = snapshots
        .current_journal_revision()
        .map_err(ProfileStageError::Snapshot)?;
    if current_revision != pending.journal_revision() {
        return Err(ProfileStageError::JournalChanged);
    }
    state
        .stage_profile(&verified)
        .map_err(ProfileStageError::Authority)
}
