use super::{
    BankError, ProfileBank, ProfileBankAuthenticator, ProfileBankMetadata, ProfileBankStorage,
};
use crate::model::material::{key_only, matches_upload_key};
use crate::{ProfileMaterial, RecoveredRecord, UnvalidatedRecoveredRecord};
use authority_protocol::{ProfileUploadIntent, RelayProfileState};

impl<S: ProfileBankStorage, A: ProfileBankAuthenticator> ProfileBank<S, A> {
    /// Authenticate every completed reference and durable upload prefix before exposing state.
    pub fn validate_recovered(
        &mut self,
        recovered: UnvalidatedRecoveredRecord,
    ) -> Result<RecoveredRecord, BankError> {
        if let Some(active) = recovered.record.active.as_ref() {
            self.validate_reference(active)?;
        }
        let mut upload_bank_complete = false;
        match recovered.record.protected.bindings().relay {
            RelayProfileState::Uploading(intent) => {
                let Some(pending) = recovered.record.pending.as_ref() else {
                    return Err(self.seal());
                };
                let Some(metadata) = upload_metadata(pending, intent) else {
                    return Err(self.seal());
                };
                if intent.complete() {
                    self.complete(&metadata, intent.next_index)?;
                    upload_bank_complete = true;
                } else {
                    self.recover_upload(&metadata, intent.next_index)?;
                }
            }
            _ => {
                if let Some(pending) = recovered.record.pending.as_ref() {
                    if !key_only(pending) {
                        self.validate_reference(pending)?;
                    }
                }
            }
        }
        Ok(RecoveredRecord {
            record: recovered.record,
            upload_bank_complete,
        })
    }
}

fn upload_metadata(
    pending: &ProfileMaterial,
    intent: ProfileUploadIntent,
) -> Option<ProfileBankMetadata> {
    matches_upload_key(pending, intent).then_some(ProfileBankMetadata {
        slot: intent.pending_slot,
        device_id: intent.device_id,
        authority_id: intent.authority_id,
        authority_epoch: intent.authority_epoch,
        boot_epoch: intent.boot_epoch,
        generation: intent.generation,
        policy_epoch: intent.policy_epoch,
        upload_handle: intent.upload_handle,
        profile_len: intent.profile_len,
        profile_digest: intent.profile_digest,
        pending_spki_digest: intent.pending_spki_digest,
        spki: pending.spki,
        tpm_public_digest: intent.tpm_public_digest,
    })
}
