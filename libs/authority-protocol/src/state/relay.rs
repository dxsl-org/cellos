use super::ProfileUploadIntent;
use crate::{StagedProfileReceipt, DIGEST_LEN, ID_LEN};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelayIntent {
    pub device_id: [u8; ID_LEN],
    pub authority_id: [u8; ID_LEN],
    pub authority_epoch: u64,
    pub generation: u64,
    pub csr_handle: u64,
    pub policy_epoch: u64,
    pub pending_slot: u8,
    pub pending_spki_digest: [u8; DIGEST_LEN],
    pub profile_digest: [u8; DIGEST_LEN],
    pub boot_epoch: u64,
    pub validation_request_id: u64,
    pub tpm_public_digest: [u8; DIGEST_LEN],
    pub upload_handle: u64,
    pub profile_len: u32,
}

impl RelayIntent {
    pub const fn staged_receipt(&self) -> StagedProfileReceipt {
        StagedProfileReceipt {
            device_id: self.device_id,
            authority_id: self.authority_id,
            authority_epoch: self.authority_epoch,
            generation: self.generation,
            policy_epoch: self.policy_epoch,
            pending_slot: self.pending_slot,
            pending_spki_digest: self.pending_spki_digest,
            profile_digest: self.profile_digest,
            boot_epoch: self.boot_epoch,
            upload_handle: self.upload_handle,
            profile_len: self.profile_len,
            validation_request_id: self.validation_request_id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreparedCommitIntent(pub(super) RelayIntent);
impl PreparedCommitIntent {
    pub const fn intent(&self) -> &RelayIntent {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayProfileState {
    Empty,
    Pending {
        generation: u64,
        csr_handle: u64,
        pending_slot: u8,
    },
    Uploading(ProfileUploadIntent),
    Staged(RelayIntent),
    ReceiptConsumed(RelayIntent),
    Prepared(RelayIntent),
    Promoted {
        intent: RelayIntent,
        provider_signature: [u8; crate::SIGNATURE_LEN],
    },
    Active(RelayIntent),
}
