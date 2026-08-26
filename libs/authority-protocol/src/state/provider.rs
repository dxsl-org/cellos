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
}
