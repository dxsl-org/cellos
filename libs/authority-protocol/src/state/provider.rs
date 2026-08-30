use super::{ProfileUploadIntent, RelayIntent};
use crate::{constant_time_eq, ProviderCasReceipt, ValidateAndStageRelayProfileRequest};

impl RelayIntent {
    pub(super) fn matches_receipt(&self, receipt: &ProviderCasReceipt) -> bool {
        constant_time_eq(&self.device_id, &receipt.device_id)
            && constant_time_eq(&self.authority_id, &receipt.authority_id)
            && self.authority_epoch == receipt.authority_epoch
            && self.generation == receipt.generation
            && self.policy_epoch == receipt.policy_epoch
            && self.pending_slot == receipt.pending_slot
            && constant_time_eq(&self.pending_spki_digest, &receipt.pending_spki_digest)
            && constant_time_eq(&self.profile_digest, &receipt.profile_digest)
            && self.boot_epoch == receipt.boot_epoch
            && self.validation_request_id == receipt.validation_request_id
            && self.upload_handle == receipt.upload_handle
            && self.profile_len == receipt.profile_len
    }

    pub(super) const fn provider_receipt(
        self,
        provider_signature: [u8; crate::SIGNATURE_LEN],
    ) -> ProviderCasReceipt {
        ProviderCasReceipt {
            device_id: self.device_id,
            authority_id: self.authority_id,
            authority_epoch: self.authority_epoch,
            generation: self.generation,
            policy_epoch: self.policy_epoch,
            pending_slot: self.pending_slot,
            pending_spki_digest: self.pending_spki_digest,
            profile_digest: self.profile_digest,
            boot_epoch: self.boot_epoch,
            validation_request_id: self.validation_request_id,
            upload_handle: self.upload_handle,
            profile_len: self.profile_len,
            provider_signature,
        }
    }

    pub(super) fn matches_profile_request(
        &self,
        request: &ValidateAndStageRelayProfileRequest,
    ) -> bool {
        self.boot_epoch == request.context.boot_epoch
            && self.generation == request.generation
            && self.policy_epoch == request.policy_epoch
            && self.pending_slot == request.pending_slot
            && constant_time_eq(&self.pending_spki_digest, &request.pending_spki_digest)
            && constant_time_eq(&self.profile_digest, &request.profile_digest)
            && constant_time_eq(&self.tpm_public_digest, &request.tpm_public_digest)
            && self.upload_handle == request.upload_handle
            && self.profile_len == request.profile_len
    }
}

impl ProfileUploadIntent {
    pub(super) fn matches_profile_request(
        &self,
        request: &ValidateAndStageRelayProfileRequest,
    ) -> bool {
        self.complete()
            && self.boot_epoch == request.context.boot_epoch
            && self.generation == request.generation
            && self.policy_epoch == request.policy_epoch
            && self.pending_slot == request.pending_slot
            && constant_time_eq(&self.pending_spki_digest, &request.pending_spki_digest)
            && constant_time_eq(&self.profile_digest, &request.profile_digest)
            && constant_time_eq(&self.tpm_public_digest, &request.tpm_public_digest)
            && self.upload_handle == request.upload_handle
            && self.profile_len == request.profile_len
    }

    pub(super) fn matches_relay_intent(&self, intent: &RelayIntent) -> bool {
        self.device_id == intent.device_id
            && self.authority_id == intent.authority_id
            && self.authority_epoch == intent.authority_epoch
            && self.boot_epoch == intent.boot_epoch
            && self.generation == intent.generation
            && self.csr_handle == intent.csr_handle
            && self.policy_epoch == intent.policy_epoch
            && self.pending_slot == intent.pending_slot
            && constant_time_eq(&self.pending_spki_digest, &intent.pending_spki_digest)
            && constant_time_eq(&self.profile_digest, &intent.profile_digest)
            && constant_time_eq(&self.tpm_public_digest, &intent.tpm_public_digest)
            && self.upload_handle == intent.upload_handle
            && self.profile_len == intent.profile_len
    }
}
