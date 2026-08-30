use authority_protocol::AdmittedProfileValidation;
use stm32_authority_journal::PendingEnrollmentSnapshot;

use crate::TrustedPolicy;

#[derive(Clone, Copy)]
pub(crate) struct AdmissionBinding {
    pub device_id: [u8; 32],
    pub authority_id: [u8; 32],
    pub authority_epoch: u64,
    pub boot_epoch: u64,
    pub csr_handle: u64,
    pub slot: u8,
    pub generation: u64,
    pub policy_epoch: u64,
    pub upload_handle: u64,
    pub profile_len: u32,
}

impl AdmissionBinding {
    pub fn from_admitted(value: &AdmittedProfileValidation) -> Self {
        let request = value.request();
        Self {
            device_id: request.context.device_id,
            authority_id: request.context.authority_id,
            authority_epoch: value.authority_epoch(),
            boot_epoch: request.context.boot_epoch,
            csr_handle: value.csr_handle(),
            slot: request.pending_slot,
            generation: request.generation,
            policy_epoch: request.policy_epoch,
            upload_handle: request.upload_handle,
            profile_len: request.profile_len,
        }
    }
}

pub(crate) fn matches(
    snapshot: &PendingEnrollmentSnapshot,
    policy: TrustedPolicy<'_>,
    admitted: AdmissionBinding,
) -> bool {
    snapshot.journal_revision() == policy.expected_journal_revision
        && snapshot.protected_revision() == policy.expected_journal_revision
        && snapshot.device_id() == &admitted.device_id
        && snapshot.authority_id() == &admitted.authority_id
        && snapshot.authority_epoch() == admitted.authority_epoch
        && snapshot.boot_epoch() == admitted.boot_epoch
        && snapshot.csr_handle() == admitted.csr_handle
        && snapshot.pending_slot() == admitted.slot
        && snapshot.generation() == admitted.generation
        && snapshot.policy_epoch() == admitted.policy_epoch
        && snapshot.upload_handle() != 0
        && snapshot.upload_handle() == admitted.upload_handle
        && snapshot.profile_len() == admitted.profile_len
        && snapshot.pending_slot() == policy.expected_slot
        && snapshot.generation() == policy.expected_generation
        && snapshot.policy_epoch() == policy.expected_policy_epoch
}
