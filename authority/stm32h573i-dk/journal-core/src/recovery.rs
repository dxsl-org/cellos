use crate::{decode_record, FullRecord, RecordAuthenticator, SlotRole};
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

/// Recover exactly one authenticated record matching the protected counter.
pub fn recover<A: RecordAuthenticator>(
    counter: u64,
    slots: [Option<&[u8]>; 2],
    authenticator: &A,
    expected: &ExpectedIdentity,
) -> Result<RecoveredRecord, RecoveryError> {
    if counter == 0 {
        return Err(RecoveryError::Sealed);
    }
    let mut recovered = None;
    for (index, bytes) in slots.into_iter().enumerate() {
        let Some(bytes) = bytes else { continue };
        let Ok(record) = decode_record(bytes, authenticator) else {
            continue;
        };
        if record.counter != counter
            || record.slot_role != role(index)
            || !identity_matches(&record, expected)
        {
            continue;
        }
        if recovered.is_some() {
            return Err(RecoveryError::Ambiguous);
        }
        recovered = Some(RecoveredRecord { record });
    }
    recovered.ok_or(RecoveryError::Sealed)
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
