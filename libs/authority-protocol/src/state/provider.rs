use super::RelayIntent;
use crate::{constant_time_eq, ProviderCasReceipt};

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
            provider_signature,
        }
    }
}
