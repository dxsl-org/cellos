use crate::{BeginRelayProfileUploadRequest, ProfileUploadIntent, PROFILE_CHUNK_MAX};

pub(super) fn upload_matches(
    request: &BeginRelayProfileUploadRequest,
    intent: &ProfileUploadIntent,
) -> bool {
    intent.boot_epoch == request.context.boot_epoch
        && intent.generation == request.generation
        && intent.policy_epoch == request.policy_epoch
        && intent.pending_slot == request.pending_slot
        && intent.pending_spki_digest == request.pending_spki_digest
        && intent.profile_digest == request.profile_digest
        && intent.tpm_public_digest == request.tpm_public_digest
        && intent.upload_handle == request.upload_handle
        && intent.profile_len == request.profile_len
}

pub(super) fn expected_chunk_len(intent: &ProfileUploadIntent, index: u8) -> Option<usize> {
    let offset = (index as usize).checked_mul(PROFILE_CHUNK_MAX)?;
    let remaining = (intent.profile_len as usize).checked_sub(offset)?;
    (remaining != 0).then_some(remaining.min(PROFILE_CHUNK_MAX))
}
