use super::support::*;
use crate::*;
use authority_protocol::WireError;
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
    version[4] = 2;
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

fn resign(bytes: &mut [u8]) {
    let body = bytes.len() - 32;
    let tag = TestAuth.authenticate(&bytes[..body]);
    bytes[body..].copy_from_slice(&tag);
}
