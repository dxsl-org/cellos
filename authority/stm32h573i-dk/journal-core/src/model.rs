pub(crate) mod material;
mod relay;

use authority_protocol::{
    verify_protected_record, Bounded, ProtectedAuthorityRecord, ProtectedRecordVerifier, DIGEST_LEN,
};
use material::validate as validate_material;
pub const SPKI_MAX: usize = 96;

struct StructuralRecord;
impl ProtectedRecordVerifier for StructuralRecord {
    fn verify(&self, _record: &ProtectedAuthorityRecord) -> bool {
        true
    }
}
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotRole {
    A = 0,
    B = 1,
}

impl SlotRole {
    pub const fn other(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HardwareBindings {
    pub lane_id: [u8; DIGEST_LEN],
    pub restart_floor: u64,
    pub approved_boot_measurement: [u8; DIGEST_LEN],
    pub approved_loader_digest: [u8; DIGEST_LEN],
    pub manifest_key_digest: [u8; DIGEST_LEN],
    pub firmware_floor: u64,
    pub policy_floor: u64,
    pub trust_digest: [u8; DIGEST_LEN],
    pub verifier_digest: [u8; DIGEST_LEN],
    pub denylist_digest: [u8; DIGEST_LEN],
    pub qualification_digest: [u8; DIGEST_LEN],
}

/// Fixed key material; nonzero profile length names an authenticated external bank.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileMaterial {
    pub device_id: [u8; DIGEST_LEN],
    pub authority_id: [u8; DIGEST_LEN],
    pub authority_epoch: u64,
    pub boot_epoch: u64,
    pub slot: u8,
    pub generation: u64,
    pub profile_len: u32,
    pub profile_digest: [u8; DIGEST_LEN],
    pub tpm_public_digest: [u8; DIGEST_LEN],
    pub spki: Bounded<SPKI_MAX>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FullRecord {
    pub counter: u64,
    pub slot_role: SlotRole,
    pub hardware: HardwareBindings,
    pub protected: ProtectedAuthorityRecord,
    pub active: Option<ProfileMaterial>,
    pub pending: Option<ProfileMaterial>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordError {
    InvalidProtectedRecord,
    InvalidCounter,
    RevisionMismatch,
    LoaderMismatch,
    IdentityMismatch,
    FloorRegression,
    InvalidSuccessor,
    InvalidProfile,
    ProfileMismatch,
}

impl FullRecord {
    /// Validate cross-layer invariants before encoding or recovery.
    pub fn validate(&self) -> Result<(), RecordError> {
        verify_protected_record(self.protected, &StructuralRecord)
            .map_err(|_| RecordError::InvalidProtectedRecord)?;
        let bindings = self.protected.bindings();
        if self.counter == 0 {
            return Err(RecordError::InvalidCounter);
        }
        if bindings.revision != self.counter {
            return Err(RecordError::RevisionMismatch);
        }
        if bindings.approved_loader_digest != self.hardware.approved_loader_digest {
            return Err(RecordError::LoaderMismatch);
        }
        validate_material(self.active.as_ref())?;
        validate_material(self.pending.as_ref())?;
        if matches!((&self.active, &self.pending), (Some(a), Some(p)) if a.slot == p.slot) {
            return Err(RecordError::InvalidProfile);
        }
        relay::validate(bindings, self.active.as_ref(), self.pending.as_ref())
    }

    /// Validate the device, authority, and lane selected by protected storage.
    pub fn validate_identity(
        &self,
        device_id: &[u8; DIGEST_LEN],
        authority_id: &[u8; DIGEST_LEN],
        lane_id: &[u8; DIGEST_LEN],
    ) -> Result<(), RecordError> {
        let bindings = self.protected.bindings();
        require_with(
            bindings.device_id == *device_id
                && bindings.authority_id == *authority_id
                && self.hardware.lane_id == *lane_id,
            RecordError::IdentityMismatch,
        )
    }
}

fn require_with(condition: bool, error: RecordError) -> Result<(), RecordError> {
    condition.then_some(()).ok_or(error)
}
