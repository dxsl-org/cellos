use super::{
    BankError, ProfileBank, ProfileBankAuthenticator, ProfileBankMetadata, ProfileBankStorage,
};
use authority_protocol::{
    AuthorityFault, AuthorityState, BeginRelayProfileUploadRequest, ProfileChunkMode,
    ProfileUploadIntent, ProtectedStore, RelayProfileState, ValidatedRequest,
    WriteRelayProfileChunkRequest,
};

/// Fail-closed error from the cross-crate protected-state/profile-bank boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadFlowError {
    /// Protected state rejected or could not persist the request.
    Authority(AuthorityFault),
    /// Profile-bank initialization, authentication, storage, or readback failed.
    Bank(BankError),
    /// Caller metadata or media progress did not match the authorized intent.
    Binding,
}

/// Initialize a new inactive bank, or authenticate an existing upload, before acknowledgement.
///
/// # Errors
///
/// Returns [`UploadFlowError::Authority`] when authorization or acknowledgement fails,
/// [`UploadFlowError::Bank`] when media initialization/readback fails, or
/// [`UploadFlowError::Binding`] when `metadata` does not exactly match the authorized intent.
pub fn begin_profile_upload<S, B, A>(
    state: &mut AuthorityState<S>,
    bank: &mut ProfileBank<B, A>,
    request: &ValidatedRequest<BeginRelayProfileUploadRequest>,
    metadata: &ProfileBankMetadata,
) -> Result<ProfileUploadIntent, UploadFlowError>
where
    S: ProtectedStore,
    B: ProfileBankStorage,
    A: ProfileBankAuthenticator,
{
    let prior_relay = state.relay_state();
    let intent = state
        .authorize_profile_upload(request)
        .map_err(UploadFlowError::Authority)?;
    if !metadata_matches(metadata, &intent) {
        let _ = state.reject_profile_upload(&intent);
        return Err(UploadFlowError::Binding);
    }
    let media = match prior_relay {
        RelayProfileState::Pending { .. } => bank.initialize(metadata),
        RelayProfileState::Uploading(previous) if previous == intent => {
            bank.recover_upload(metadata, intent.next_index).map(|_| ())
        }
        _ => {
            let _ = state.reject_profile_upload(&intent);
            return Err(UploadFlowError::Binding);
        }
    };
    if let Err(error) = media {
        let _ = state.reject_profile_upload(&intent);
        return Err(UploadFlowError::Bank(error));
    }
    state
        .acknowledge_profile_upload(&intent)
        .map_err(UploadFlowError::Authority)
}
/// Authenticate and read back one bank chunk before advancing protected progress.
///
/// # Errors
///
/// Returns [`UploadFlowError::Authority`] when authorization or acknowledgement fails,
/// [`UploadFlowError::Bank`] when the authenticated write/readback fails, or
/// [`UploadFlowError::Binding`] when metadata or returned media progress diverges from the
/// authorized intent.
pub fn write_profile_chunk<S, B, A>(
    state: &mut AuthorityState<S>,
    bank: &mut ProfileBank<B, A>,
    request: &ValidatedRequest<WriteRelayProfileChunkRequest>,
    metadata: &ProfileBankMetadata,
) -> Result<ProfileUploadIntent, UploadFlowError>
where
    S: ProtectedStore,
    B: ProfileBankStorage,
    A: ProfileBankAuthenticator,
{
    let intent = state
        .authorize_profile_chunk(request)
        .map_err(UploadFlowError::Authority)?;
    if !metadata_matches(metadata, &intent.upload) {
        let _ = state.reject_profile_chunk(&intent);
        return Err(UploadFlowError::Binding);
    }
    let next_index = match bank.write_chunk(
        metadata,
        intent.upload.next_index,
        intent.chunk_index,
        intent.chunk.as_slice(),
    ) {
        Ok(value) => value,
        Err(error) => {
            let _ = state.reject_profile_chunk(&intent);
            return Err(UploadFlowError::Bank(error));
        }
    };
    let expected = match intent.mode {
        ProfileChunkMode::VerifyExisting => intent.upload.next_index,
        ProfileChunkMode::Write => intent.upload.next_index + 1,
    };
    if next_index != expected {
        let _ = state.reject_profile_chunk(&intent);
        return Err(UploadFlowError::Binding);
    }
    state
        .acknowledge_profile_chunk(&intent)
        .map_err(UploadFlowError::Authority)
}

fn metadata_matches(metadata: &ProfileBankMetadata, intent: &ProfileUploadIntent) -> bool {
    metadata.slot == intent.pending_slot
        && metadata.device_id == intent.device_id
        && metadata.authority_id == intent.authority_id
        && metadata.authority_epoch == intent.authority_epoch
        && metadata.boot_epoch == intent.boot_epoch
        && metadata.generation == intent.generation
        && metadata.policy_epoch == intent.policy_epoch
        && metadata.upload_handle == intent.upload_handle
        && metadata.profile_len == intent.profile_len
        && metadata.profile_digest == intent.profile_digest
        && metadata.pending_spki_digest == intent.pending_spki_digest
        && metadata.tpm_public_digest == intent.tpm_public_digest
}
