use super::support::*;
use crate::*;
use authority_protocol::{Bounded, WireError};
use sha2::{Digest, Sha256};
use std::vec::Vec;

#[test]
fn canonical_record_round_trips_exactly() {
    let bytes = encoded(SlotRole::A);
    let decoded = decode_record(&bytes, &TestAuth).unwrap();
    assert_eq!(decoded, full_record(SlotRole::A));
    let mut output = [0u8; RECORD_MAX];
    let length = encode_record(&decoded, &TestAuth, &mut output).unwrap();
    assert_eq!(&output[..length], bytes.as_slice());
}
#[test]
fn bank_references_round_trip_without_profile_bytes() {
    let record = active_record();
    let bytes = encode_full(&record);
    assert!(bytes.len() < RECORD_MAX);
    assert_eq!(decode_record(&bytes, &TestAuth), Ok(record.clone()));
    let mut replay = record;
    replay.active.as_mut().unwrap().authority_epoch += 1;
    let mut output = [0u8; RECORD_MAX];
    assert_eq!(
        encode_record(&replay, &TestAuth, &mut output),
        Err(CodecError::Record(RecordError::ProfileMismatch))
    );
}

#[test]
fn authentication_precedes_hostile_body_parsing() {
    let mut bytes = encoded(SlotRole::A);
    bytes[0] ^= 1;
    assert_eq!(
        decode_record(&bytes, &TestAuth),
        Err(CodecError::Authentication)
    );
}

#[test]
fn reserved_version_and_trailing_bytes_fail_closed() {
    let mut reserved = encoded(SlotRole::A);
    reserved[7] = 1;
    resign(&mut reserved);
    assert_eq!(
        decode_record(&reserved, &TestAuth),
        Err(CodecError::Wire(WireError::NonZeroReserved))
    );

    let mut version = encoded(SlotRole::A);
    version[4] = 1;
    resign(&mut version);
    assert_eq!(
        decode_record(&version, &TestAuth),
        Err(CodecError::Wire(WireError::UnsupportedVersion))
    );

    let original = encoded(SlotRole::A);
    let body = original.len() - 32;
    let mut trailing = Vec::from(&original[..body]);
    trailing.push(0);
    trailing.extend_from_slice(&TestAuth.authenticate(&trailing));
    assert_eq!(
        decode_record(&trailing, &TestAuth),
        Err(CodecError::Wire(WireError::TrailingBytes))
    );
}

#[test]
fn counter_revision_and_loader_bindings_are_mandatory() {
    let mut output = [0u8; RECORD_MAX];
    let mut record = full_record(SlotRole::A);
    record.counter = 0;
    assert_eq!(
        encode_record(&record, &TestAuth, &mut output),
        Err(CodecError::Record(RecordError::InvalidCounter))
    );
    record.counter = 2;
    assert_eq!(
        encode_record(&record, &TestAuth, &mut output),
        Err(CodecError::Record(RecordError::RevisionMismatch))
    );
    record.counter = 1;
    record.hardware.approved_loader_digest = [0; 32];
    assert_eq!(
        encode_record(&record, &TestAuth, &mut output),
        Err(CodecError::Record(RecordError::LoaderMismatch))
    );
}
pub fn active_record() -> FullRecord {
    active_record_at(1)
}

pub fn active_record_at(generation: u64) -> FullRecord {
    let mut record = full_record(SlotRole::A);
    let mut fixed = [0u8; authority_protocol::PROTECTED_RECORD_MAX];
    let length = record.protected.encode_canonical(&mut fixed).unwrap();
    let mut bytes = fixed[..length].to_vec();
    let spki = b"bank-spki";
    let profile_digest: [u8; 32] = Sha256::digest([0x5a; 100]).into();
    let mut active = Vec::new();
    active.push(7);
    active.extend_from_slice(&[1; 32]);
    active.extend_from_slice(&[2; 32]);
    active.extend_from_slice(&1u64.to_le_bytes());
    active.extend_from_slice(&generation.to_le_bytes());
    active.extend_from_slice(&generation.to_le_bytes());
    active.extend_from_slice(&1u64.to_le_bytes());
    active.push(0);
    active.extend_from_slice(&Sha256::digest(spki));
    active.extend_from_slice(&profile_digest);
    active.extend_from_slice(&1u64.to_le_bytes());
    active.extend_from_slice(&44u64.to_le_bytes());
    active.extend_from_slice(&[23; 32]);
    active.extend_from_slice(&9u64.to_le_bytes());
    active.extend_from_slice(&100u32.to_le_bytes());
    let shift = active.len() - 1;
    bytes.splice(24..25, active);
    bytes[105 + shift..113 + shift].copy_from_slice(&generation.to_le_bytes());
    record.protected =
        authority_protocol::ProtectedAuthorityRecord::decode_canonical(&bytes).unwrap();
    record.active = Some(ProfileMaterial {
        device_id: [1; 32],
        authority_id: [2; 32],
        authority_epoch: 1,
        boot_epoch: 1,
        slot: 0,
        generation,
        profile_len: 100,
        profile_digest,
        tpm_public_digest: [23; 32],
        spki: Bounded::from_slice(spki).unwrap(),
    });
    record
}

fn resign(bytes: &mut [u8]) {
    let body = bytes.len() - 32;
    let tag = TestAuth.authenticate(&bytes[..body]);
    bytes[body..].copy_from_slice(&tag);
}
