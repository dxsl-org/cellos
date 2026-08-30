use super::media::*;
use super::support::*;
use crate::*;

fn counter(fail_increment: bool) -> FakeCounter {
    FakeCounter {
        value: 0,
        fail_increment,
        sealed: false,
        events: std::vec::Vec::new(),
    }
}

#[test]
fn successful_commit_increments_then_writes_and_reads_back() {
    let mut journal = Journal::new(
        counter(false),
        FakeStorage::empty(StorageFault::None),
        TestAuth,
        identity(),
    );
    let recovered = journal.commit(full_record(SlotRole::B)).unwrap();
    assert_eq!(recovered.record().counter, 1);
    assert_eq!(recovered.record().slot_role, SlotRole::A);
    let (counter, storage, _) = journal.into_parts();
    assert_eq!(counter.value, 1);
    assert_eq!(
        counter.events,
        [Event::CounterRead, Event::CounterIncrement]
    );
    assert_eq!(
        storage.events,
        [
            Event::Erase(SlotRole::A),
            Event::Write(SlotRole::A),
            Event::Read(SlotRole::A)
        ]
    );
}

#[test]
fn invalid_record_is_rejected_before_counter_increment() {
    let mut record = full_record(SlotRole::A);
    record.hardware.approved_loader_digest = [0; 32];
    let mut journal = Journal::new(
        counter(false),
        FakeStorage::empty(StorageFault::None),
        TestAuth,
        identity(),
    );
    assert_eq!(journal.commit(record), Err(JournalError::InvalidRecord));
    let (counter, storage, _) = journal.into_parts();
    assert_eq!(counter.value, 0);
    assert_eq!(counter.events, [Event::CounterRead]);
    assert!(storage.events.is_empty());
}

#[test]
fn failed_increment_never_touches_a_slot() {
    let mut journal = Journal::new(
        counter(true),
        FakeStorage::empty(StorageFault::None),
        TestAuth,
        identity(),
    );
    assert_eq!(
        journal.commit(full_record(SlotRole::A)),
        Err(JournalError::Sealed)
    );
    let (counter, storage, _) = journal.into_parts();
    assert_eq!(counter.value, 0);
    assert!(counter.sealed);
    assert_eq!(
        counter.events,
        [
            Event::CounterRead,
            Event::CounterIncrement,
            Event::CounterSeal
        ]
    );
    assert!(storage.events.is_empty());
}

#[test]
fn every_post_increment_storage_cut_seals() {
    for fault in [
        StorageFault::Erase,
        StorageFault::Write,
        StorageFault::PartialWrite,
        StorageFault::CorruptRead,
    ] {
        let mut journal = Journal::new(
            counter(false),
            FakeStorage::empty(fault),
            TestAuth,
            identity(),
        );
        assert_eq!(
            journal.commit(full_record(SlotRole::A)),
            Err(JournalError::Sealed)
        );
        let (counter, mut storage, _) = journal.into_parts();
        assert_eq!(counter.value, 1);
        assert!(counter.sealed);
        storage.fault = StorageFault::None;
        let mut rebooted = Journal::new(counter, storage, TestAuth, identity());
        assert_eq!(rebooted.recover(), Err(JournalError::Sealed));
    }
}

#[test]
fn storage_supplied_oversize_length_seals_without_indexing() {
    let mut journal = Journal::new(
        FakeCounter {
            value: 1,
            fail_increment: false,
            sealed: false,
            events: std::vec::Vec::new(),
        },
        FakeStorage::empty(StorageFault::BadLength),
        TestAuth,
        identity(),
    );
    assert_eq!(journal.recover(), Err(JournalError::Sealed));
    let (counter, _, _) = journal.into_parts();
    assert!(counter.sealed);
}
