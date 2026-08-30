use super::media::*;
use super::support::*;
use crate::*;
use sha2::{Digest, Sha256};

fn counter(value: u64) -> FakeCounter {
    FakeCounter {
        value,
        fail_increment: false,
        sealed: false,
        events: std::vec::Vec::new(),
    }
}

fn current_storage() -> FakeStorage {
    let mut storage = FakeStorage::empty(StorageFault::None);
    storage.slots[0] = encoded(SlotRole::A);
    storage
}

#[test]
fn wrong_identity_is_rejected_before_counter_increment() {
    let mut wrong = identity();
    wrong.device_id = [9; 32];
    let mut journal = Journal::new(
        counter(0),
        FakeStorage::empty(StorageFault::None),
        TestAuth,
        wrong,
    );
    assert_eq!(
        journal.commit(full_record(SlotRole::A)),
        Err(JournalError::InvalidRecord)
    );
    let (counter, storage, _) = journal.into_parts();
    assert_eq!(counter.value, 0);
    assert_eq!(counter.events, [Event::CounterRead]);
    assert!(storage.events.is_empty());
}

#[test]
fn successor_floors_and_immutable_digests_cannot_regress() {
    let current = full_record(SlotRole::A);
    for mutate in [
        |record: &mut FullRecord| record.hardware.restart_floor = 0,
        |record: &mut FullRecord| record.hardware.manifest_key_digest = [0; 32],
    ] {
        let mut next = successor(&current);
        mutate(&mut next);
        let mut journal = Journal::new(counter(1), current_storage(), TestAuth, identity());
        assert_eq!(journal.commit(next), Err(JournalError::InvalidRecord));
        let (counter, storage, _) = journal.into_parts();
        assert_eq!(counter.value, 1);
        assert_eq!(counter.events, [Event::CounterRead, Event::CounterRead]);
        assert_eq!(
            storage.events,
            [Event::Read(SlotRole::A), Event::Read(SlotRole::B)]
        );
    }
}

#[test]
fn legal_successor_uses_the_inactive_slot() {
    let current = full_record(SlotRole::A);
    let next = successor(&current);
    assert_eq!(next.validate(), Ok(()));
    assert_eq!(next.validate_successor(Some(&current)), Ok(()));
    let mut journal = Journal::new(counter(1), current_storage(), TestAuth, identity());
    let recovered = journal.commit(next).unwrap();
    assert_eq!(recovered.record().counter, 2);
    assert_eq!(recovered.record().slot_role, SlotRole::B);
}

#[test]
fn pending_and_uploading_require_the_exact_key_material() {
    let pending = pending_record();
    assert_eq!(pending.validate(), Ok(()));
    let previous = full_record(SlotRole::A);
    assert_eq!(pending.validate_successor(Some(&previous)), Ok(()));

    let mut premature = pending.clone();
    let material = premature.pending.as_mut().unwrap();
    material.profile_len = 1;
    material.profile_digest = [3; 32];
    assert_eq!(premature.validate(), Err(RecordError::ProfileMismatch));
    let mut partial = pending.clone();
    partial.pending.as_mut().unwrap().profile_digest = [1; 32];
    assert_eq!(partial.validate(), Err(RecordError::InvalidProfile));

    let mut missing = pending.clone();
    missing.pending = None;
    assert_eq!(missing.validate(), Err(RecordError::ProfileMismatch));

    let uploading = uploading_record(&pending);
    assert_eq!(uploading.validate(), Ok(()));
    assert_eq!(uploading.validate_successor(Some(&pending)), Ok(()));
    assert_eq!(uploading.pending, pending.pending);

    let mut substituted = uploading;
    substituted.pending.as_mut().unwrap().tpm_public_digest = [99; 32];
    assert_eq!(substituted.validate(), Err(RecordError::ProfileMismatch));
}

pub const UPLOAD_PROFILE: [u8; 100] = [0x5a; 100];

pub fn uploading_record(pending: &FullRecord) -> FullRecord {
    upload_record(pending, 3, 0, 18)
}

pub fn completed_uploading_record(uploading: &FullRecord) -> FullRecord {
    upload_record(uploading, 4, 1, upload_bytes(0).len())
}

fn upload_record(
    previous: &FullRecord,
    counter: u64,
    next_index: u8,
    previous_relay_len: usize,
) -> FullRecord {
    let mut fixed = [0u8; authority_protocol::PROTECTED_RECORD_MAX];
    let length = previous.protected.encode_canonical(&mut fixed).unwrap();
    let mut bytes = fixed[..length].to_vec();
    bytes[5..13].copy_from_slice(&counter.to_le_bytes());
    bytes.splice(24..24 + previous_relay_len, upload_bytes(next_index));
    let protected = authority_protocol::ProtectedAuthorityRecord::decode_canonical(&bytes).unwrap();
    FullRecord {
        counter,
        slot_role: previous.slot_role.other(),
        protected,
        ..previous.clone()
    }
}

fn upload_bytes(next_index: u8) -> std::vec::Vec<u8> {
    let mut upload = std::vec::Vec::new();
    upload.push(2);
    upload.extend_from_slice(&[1; 32]);
    upload.extend_from_slice(&[2; 32]);
    upload.extend_from_slice(&1u64.to_le_bytes());
    upload.extend_from_slice(&1u64.to_le_bytes());
    upload.extend_from_slice(&1u64.to_le_bytes());
    upload.extend_from_slice(&1u64.to_le_bytes());
    upload.extend_from_slice(&1u64.to_le_bytes());
    upload.push(0);
    upload.extend_from_slice(&Sha256::digest(b"pending-spki"));
    upload.extend_from_slice(&Sha256::digest(UPLOAD_PROFILE));
    upload.extend_from_slice(&[13; 32]);
    upload.extend_from_slice(&9u64.to_le_bytes());
    upload.extend_from_slice(&(UPLOAD_PROFILE.len() as u32).to_le_bytes());
    upload.push(next_index);
    upload
}

#[test]
fn counter_exhaustion_irreversibly_seals() {
    let prior = record_at(u64::MAX - 1, SlotRole::A);
    let current = record_at(u64::MAX, SlotRole::B);
    let mut storage = FakeStorage::empty(StorageFault::None);
    storage.slots[0] = encode_full(&prior);
    storage.slots[1] = encode_full(&current);
    let mut journal = Journal::new(counter(u64::MAX), storage, TestAuth, identity());
    assert_eq!(
        journal.commit(full_record(SlotRole::B)),
        Err(JournalError::Sealed)
    );
    let (counter, mut storage, _) = journal.into_parts();
    assert!(counter.sealed);
    storage.fault = StorageFault::None;
    let mut rebooted = Journal::new(counter, storage, TestAuth, identity());
    assert_eq!(rebooted.recover(), Err(JournalError::Sealed));
}
