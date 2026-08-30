use crate::{
    codec::{decode_authenticated_record, record_authenticates},
    FullRecord, RecordAuthenticator, SlotRole,
};
use authority_protocol::DIGEST_LEN;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpectedIdentity {
    pub device_id: [u8; DIGEST_LEN],
    pub authority_id: [u8; DIGEST_LEN],
    pub lane_id: [u8; DIGEST_LEN],
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveredRecord {
    pub(crate) record: FullRecord,
}

impl RecoveredRecord {
    /// Return the authenticated full record carried by this recovery token.
    pub const fn record(&self) -> &FullRecord {
        &self.record
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryError {
    Sealed,
    Ambiguous,
}

/// Recover one authenticated counter head and its exact predecessor chain.
pub fn recover<A: RecordAuthenticator>(
    counter: u64,
    slots: [Option<&[u8]>; 2],
    authenticator: &A,
    expected: &ExpectedIdentity,
) -> Result<RecoveredRecord, RecoveryError> {
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
        return Ok(RecoveredRecord { record: current });
    }
    let prior = prior.ok_or(RecoveryError::Sealed)?;
    current
        .validate_successor(Some(&prior))
        .map_err(|_| RecoveryError::Sealed)?;
    Ok(RecoveredRecord { record: current })
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
