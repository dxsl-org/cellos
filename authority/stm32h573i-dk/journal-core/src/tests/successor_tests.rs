use super::media::*;
use super::support::*;
use crate::*;

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
fn pending_state_requires_pending_material_and_preserves_no_previous_active() {
    let pending = pending_record();
    assert_eq!(pending.validate(), Ok(()));
    let previous = full_record(SlotRole::A);
    assert_eq!(pending.validate_successor(Some(&previous)), Ok(()));

    let mut profiled_early = pending.clone();
    profiled_early.pending.as_mut().unwrap().profile =
        authority_protocol::Bounded::from_slice(b"not-yet-validated").unwrap();
    assert_eq!(profiled_early.validate(), Ok(()));
    assert_eq!(
        profiled_early.validate_successor(Some(&previous)),
        Err(RecordError::InvalidSuccessor)
    );

    let mut missing = pending.clone();
    missing.pending = None;
    assert_eq!(missing.validate(), Err(RecordError::ProfileMismatch));

    let mut substituted = pending;
    let mut active = substituted.pending.clone().unwrap();
    active.slot = 1;
    substituted.active = Some(active);
    assert_eq!(substituted.validate(), Err(RecordError::ProfileMismatch));
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
