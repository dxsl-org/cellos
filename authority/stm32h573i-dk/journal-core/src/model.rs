use authority_protocol::{
    verify_protected_record, Bounded, ProtectedAuthorityRecord, ProtectedRecordVerifier,
    RelayIntent, RelayProfileState, DIGEST_LEN, PROFILE_MAX,
};
use sha2::{Digest, Sha256};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileMaterial {
    pub slot: u8,
    pub spki: Bounded<SPKI_MAX>,
    pub profile: Bounded<PROFILE_MAX>,
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
        validate_relay(
            bindings.relay,
            bindings.previous_active,
            self.active.as_ref(),
            self.pending.as_ref(),
        )
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

fn validate_material(value: Option<&ProfileMaterial>) -> Result<(), RecordError> {
    if matches!(value, Some(v) if v.slot > 1 || v.spki.is_empty()) {
        return Err(RecordError::InvalidProfile);
    }
    Ok(())
}

fn validate_relay(
    relay: RelayProfileState,
    previous: Option<RelayIntent>,
    active: Option<&ProfileMaterial>,
    pending: Option<&ProfileMaterial>,
) -> Result<(), RecordError> {
    match relay {
        RelayProfileState::Empty => {
            require(previous.is_none() && active.is_none() && pending.is_none())
        }
        RelayProfileState::Pending { .. } => {
            require(pending.is_some())?;
            match previous {
                Some(value) => match_intent(value, active),
                None => require(active.is_none()),
            }
        }
        RelayProfileState::Staged(intent)
        | RelayProfileState::ReceiptConsumed(intent)
        | RelayProfileState::Prepared(intent)
        | RelayProfileState::Promoted { intent, .. } => {
            match_intent(intent, pending)?;
            match previous {
                Some(value) => match_intent(value, active),
                None => require(active.is_none()),
            }
        }
        RelayProfileState::Active(intent) => {
            require(previous.is_none())?;
            match_intent(intent, active)?;
            require(pending.is_none())
        }
    }
}

fn match_intent(
    intent: RelayIntent,
    material: Option<&ProfileMaterial>,
) -> Result<(), RecordError> {
    let value = material.ok_or(RecordError::ProfileMismatch)?;
    require(value.slot == intent.pending_slot)
        .and_then(|_| require(digest(value.spki.as_slice()) == intent.pending_spki_digest))
        .and_then(|_| require(digest(value.profile.as_slice()) == intent.profile_digest))
}

fn require(condition: bool) -> Result<(), RecordError> {
    condition.then_some(()).ok_or(RecordError::ProfileMismatch)
}

fn require_with(condition: bool, error: RecordError) -> Result<(), RecordError> {
    condition.then_some(()).ok_or(error)
}

fn digest(value: &[u8]) -> [u8; DIGEST_LEN] {
    Sha256::digest(value).into()
}
