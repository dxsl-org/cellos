use crate::{
    codec::{decode_authenticated_record, record_authenticates},
    FullRecord, RecordAuthenticator, SlotRole,
};
use authority_protocol::DIGEST_LEN;
use core::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpectedIdentity {
    pub device_id: [u8; DIGEST_LEN],
    pub authority_id: [u8; DIGEST_LEN],
    pub lane_id: [u8; DIGEST_LEN],
}
/// Journal-authenticated state that is not serviceable until profile-bank validation.
#[derive(Clone, PartialEq, Eq)]
pub struct UnvalidatedRecoveredRecord {
    pub(crate) record: FullRecord,
}

impl UnvalidatedRecoveredRecord {
    pub(crate) const fn record(&self) -> &FullRecord {
        &self.record
    }
}

impl fmt::Debug for UnvalidatedRecoveredRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("UnvalidatedRecoveredRecord(..)")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveredRecord {
    pub(crate) record: FullRecord,
    pub(crate) upload_bank_complete: bool,
}

impl RecoveredRecord {
    /// Return the journal and profile-bank authenticated full record.
    pub const fn record(&self) -> &FullRecord {
        &self.record
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryError {
    Sealed,
    Ambiguous,
}

/// Recover one journal-authenticated head for subsequent profile-bank validation.
pub fn recover<A: RecordAuthenticator>(
    counter: u64,
    slots: [Option<&[u8]>; 2],
    authenticator: &A,
    expected: &ExpectedIdentity,
) -> Result<UnvalidatedRecoveredRecord, RecoveryError> {
    if counter == 0 {
        return Err(RecoveryError::Sealed);
    }
    let prior_counter = counter.checked_sub(1);
    let mut current = None;
    let mut prior = None;
    for (index, bytes) in slots.into_iter().enumerate() {
        let Some(bytes) = bytes else { continue };
        let authenticated = record_authenticates(bytes, authenticator).unwrap_or(false);
        if !authenticated {
            continue;
        }
        let record = decode_authenticated_record(bytes).map_err(|_| RecoveryError::Sealed)?;
        if record.slot_role != role(index) || !identity_matches(&record, expected) {
            return Err(RecoveryError::Sealed);
        }
        if record.counter == counter {
            insert_unique(&mut current, record)?;
        } else if counter > 1 && Some(record.counter) == prior_counter {
            insert_unique(&mut prior, record)?;
        } else {
            return Err(RecoveryError::Sealed);
        }
    }
    let current = current.ok_or(RecoveryError::Sealed)?;
    if counter == 1 {
        current
            .validate_successor(None)
            .map_err(|_| RecoveryError::Sealed)?;
        return Ok(UnvalidatedRecoveredRecord { record: current });
    }
    let prior = prior.ok_or(RecoveryError::Sealed)?;
    current
        .validate_successor(Some(&prior))
        .map_err(|_| RecoveryError::Sealed)?;
    Ok(UnvalidatedRecoveredRecord { record: current })
}

fn insert_unique(target: &mut Option<FullRecord>, record: FullRecord) -> Result<(), RecoveryError> {
    if target.is_some() {
        return Err(RecoveryError::Ambiguous);
    }
    *target = Some(record);
    Ok(())
}

fn identity_matches(record: &FullRecord, expected: &ExpectedIdentity) -> bool {
    let bindings = record.protected.bindings();
    bindings.device_id == expected.device_id
        && bindings.authority_id == expected.authority_id
        && record.hardware.lane_id == expected.lane_id
}

const fn role(index: usize) -> SlotRole {
    if index == 0 {
        SlotRole::A
    } else {
        SlotRole::B
    }
}
