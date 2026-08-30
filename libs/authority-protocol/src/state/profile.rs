use super::{AuthorityState, ProtectedStore, RelayIntent, RelayProfileState};
use crate::{
    AdmittedProfileValidation, AuthorityFault, RootValidatedProfile,
    ValidateAndStageRelayProfileRequest, ValidatedRequest,
};

impl<S: ProtectedStore> AuthorityState<S> {
    pub fn admit_profile_validation(
        &mut self,
        validated: &ValidatedRequest<ValidateAndStageRelayProfileRequest>,
    ) -> Result<AdmittedProfileValidation, AuthorityFault> {
        let request = validated.request();
        self.authorize_context(&request.context)?;
        let csr_handle = match self.relay {
            RelayProfileState::Uploading(upload) if upload.matches_profile_request(request) => {
                upload.csr_handle
            }
            RelayProfileState::Staged(intent) if intent.matches_profile_request(request) => {
                intent.csr_handle
            }
            RelayProfileState::Uploading(_) | RelayProfileState::Staged(_) => {
                return self.seal(AuthorityFault::ProfileRejected);
            }
            _ => return self.seal(AuthorityFault::InvalidState),
        };
        self.persist()?;
        Ok(AdmittedProfileValidation::new(
            *validated,
            self.authority_epoch,
            csr_handle,
        ))
    }

    pub fn stage_profile(
        &mut self,
        verified: &RootValidatedProfile,
    ) -> Result<RelayIntent, AuthorityFault> {
        let request = verified.request();
        let upload = match self.relay {
            RelayProfileState::Uploading(value) if value.matches_profile_request(request) => value,
            RelayProfileState::Staged(intent) if intent.matches_profile_request(request) => {
                return Ok(intent);
            }
            RelayProfileState::Uploading(_) | RelayProfileState::Staged(_) => {
                return self.seal(AuthorityFault::ProfileRejected);
            }
            _ => return self.seal(AuthorityFault::InvalidState),
        };
        let intent = RelayIntent {
            device_id: self.device_id,
            authority_id: self.authority_id,
            authority_epoch: self.authority_epoch,
            generation: upload.generation,
            csr_handle: upload.csr_handle,
            policy_epoch: upload.policy_epoch,
            pending_slot: upload.pending_slot,
            pending_spki_digest: upload.pending_spki_digest,
            profile_digest: upload.profile_digest,
            boot_epoch: upload.boot_epoch,
            validation_request_id: request.context.request_id,
            tpm_public_digest: upload.tpm_public_digest,
            upload_handle: upload.upload_handle,
            profile_len: upload.profile_len,
        };
        self.relay = RelayProfileState::Staged(intent);
        self.persist_value(intent)
    }
}
