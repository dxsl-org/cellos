// SPDX-License-Identifier: MPL-2.0
//! Owner-signed atomic A/B slot record format and parser.

use super::{BackendIdentity, FloorState, IntentDigest, SlotId, SlotObservation, TransactionId};
use alloc::vec::Vec;

pub const SLOT_MAGIC: &[u8; 16] = b"VI_OWNER_SLOT_V1";
pub const SLOT_VERSION_1: u16 = 1;
pub const FLAG_COMMITTED: u16 = 0x0001;

pub const HEADER_SIZE: usize = 144;
pub const ENTRY_SIZE: usize = 40;
pub const SIG_SIZE: usize = 64;
pub const MIN_SLOT_SIZE: usize = HEADER_SIZE + SIG_SIZE; // 208
pub const MAX_ADMISSIONS: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SlotParseError {
    TooShort,
    BadMagic,
    UnsupportedVersion,
    InvalidSlotId,
    TooManyAdmissions,
    LengthMismatch,
    SignatureInvalid,
    DuplicateAdmission,
    ConflictingGeneration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedSlot {
    pub(crate) slot_id: SlotId,
    pub is_committed: bool,
    pub(crate) floor_state: FloorState,
    pub expected_generation: u64,
    pub provenance_envelope_digest: [u8; 32],
    pub admitted_elfs: Vec<[u8; 32]>,
}

impl ParsedSlot {
    #[allow(dead_code)]
    pub(crate) fn observation(&self) -> SlotObservation {
        if self.is_committed {
            SlotObservation::AuthenticatedCommitted(self.floor_state)
        } else {
            SlotObservation::AuthenticatedIntent(self.floor_state)
        }
    }

    pub fn admits_elf(&self, elf_sha256: &[u8; 32]) -> bool {
        self.admitted_elfs
            .iter()
            .any(|admitted| admitted == elf_sha256)
    }
}

pub fn parse_slot(data: &[u8]) -> Result<ParsedSlot, SlotParseError> {
    if data.len() < MIN_SLOT_SIZE {
        return Err(SlotParseError::TooShort);
    }
    if &data[0..16] != SLOT_MAGIC {
        return Err(SlotParseError::BadMagic);
    }

    let version = u16::from_le_bytes(data[16..18].try_into().unwrap());
    if version != SLOT_VERSION_1 {
        return Err(SlotParseError::UnsupportedVersion);
    }

    // Authenticate signature before parsing variable components
    let payload = &data[..data.len() - SIG_SIZE];
    let sig: &[u8; 64] = data[data.len() - SIG_SIZE..].try_into().unwrap();
    if !super::owner_key::verify_owner_signature(payload, sig) {
        return Err(SlotParseError::SignatureInvalid);
    }

    let flags = u16::from_le_bytes(data[18..20].try_into().unwrap());
    let is_committed = (flags & FLAG_COMMITTED) != 0;

    let slot_id = match data[20] {
        0 => SlotId::A,
        1 => SlotId::B,
        _ => return Err(SlotParseError::InvalidSlotId),
    };

    let generation = u64::from_le_bytes(data[24..32].try_into().unwrap());
    let expected_generation = u64::from_le_bytes(data[32..40].try_into().unwrap());
    if expected_generation >= generation && generation > 0 {
        return Err(SlotParseError::ConflictingGeneration);
    }

    let mut transaction_id: TransactionId = [0u8; 16];
    transaction_id.copy_from_slice(&data[40..56]);

    let mut backend_identity: BackendIdentity = [0u8; 16];
    backend_identity.copy_from_slice(&data[56..72]);

    let mut intent_digest: IntentDigest = [0u8; 32];
    intent_digest.copy_from_slice(&data[72..104]);

    let mut provenance_envelope_digest = [0u8; 32];
    provenance_envelope_digest.copy_from_slice(&data[104..136]);

    let admission_count = u32::from_le_bytes(data[136..140].try_into().unwrap()) as usize;
    if admission_count > MAX_ADMISSIONS {
        return Err(SlotParseError::TooManyAdmissions);
    }

    let expected_len = HEADER_SIZE + admission_count * ENTRY_SIZE + SIG_SIZE;
    if data.len() != expected_len {
        return Err(SlotParseError::LengthMismatch);
    }

    let mut admitted_elfs = Vec::with_capacity(admission_count);
    let mut offset = HEADER_SIZE;
    for _ in 0..admission_count {
        let mut elf_hash = [0u8; 32];
        elf_hash.copy_from_slice(&data[offset..offset + 32]);
        if admitted_elfs.contains(&elf_hash) {
            return Err(SlotParseError::DuplicateAdmission);
        }
        admitted_elfs.push(elf_hash);
        offset += ENTRY_SIZE;
    }

    Ok(ParsedSlot {
        slot_id,
        is_committed,
        floor_state: FloorState {
            generation,
            transaction_id,
            intent_digest,
            backend_identity,
        },
        expected_generation,
        provenance_envelope_digest,
        admitted_elfs,
    })
}
