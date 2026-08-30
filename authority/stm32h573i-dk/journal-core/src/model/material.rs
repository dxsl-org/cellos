use super::{ProfileMaterial, RecordError};
use authority_protocol::{ProfileUploadIntent, RelayIntent, DIGEST_LEN, PROFILE_MAX_LEN};
use sha2::{Digest, Sha256};

pub(super) fn validate(value: Option<&ProfileMaterial>) -> Result<(), RecordError> {
    if matches!(value, Some(value) if !structurally_valid(value)) {
        return Err(RecordError::InvalidProfile);
    }
    Ok(())
}

pub(crate) fn key_only(value: &ProfileMaterial) -> bool {
    value.profile_len == 0 && value.profile_digest == [0; DIGEST_LEN]
}

pub(crate) fn matches_upload_key(value: &ProfileMaterial, intent: ProfileUploadIntent) -> bool {
    key_only(value)
        && value.device_id == intent.device_id
        && value.authority_id == intent.authority_id
        && value.authority_epoch == intent.authority_epoch
        && value.boot_epoch == intent.boot_epoch
        && value.generation == intent.generation
        && value.slot == intent.pending_slot
        && digest(value) == intent.pending_spki_digest
        && value.tpm_public_digest == intent.tpm_public_digest
}

pub(crate) fn matches_intent(value: &ProfileMaterial, intent: RelayIntent) -> bool {
    !key_only(value)
        && value.device_id == intent.device_id
        && value.authority_id == intent.authority_id
        && value.authority_epoch == intent.authority_epoch
        && value.boot_epoch == intent.boot_epoch
        && value.generation == intent.generation
        && value.slot == intent.pending_slot
        && digest(value) == intent.pending_spki_digest
        && value.tpm_public_digest == intent.tpm_public_digest
        && value.profile_len == intent.profile_len
        && value.profile_digest == intent.profile_digest
}

pub(crate) fn staged_from_key(old: &ProfileMaterial, new: &ProfileMaterial) -> bool {
    key_only(old)
        && !key_only(new)
        && old.device_id == new.device_id
        && old.authority_id == new.authority_id
        && old.authority_epoch == new.authority_epoch
        && old.boot_epoch == new.boot_epoch
        && old.slot == new.slot
        && old.generation == new.generation
        && old.tpm_public_digest == new.tpm_public_digest
        && old.spki == new.spki
}

fn structurally_valid(value: &ProfileMaterial) -> bool {
    let valid_profile = key_only(value)
        || (value.profile_len as usize <= PROFILE_MAX_LEN && value.profile_len != 0);
    value.slot <= 1 && value.generation != 0 && !value.spki.is_empty() && valid_profile
}

fn digest(value: &ProfileMaterial) -> [u8; DIGEST_LEN] {
    Sha256::digest(value.spki.as_slice()).into()
}
