extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;

use crate::storage::{
    load_for_tests, persist_placeholder_for_tests, JournalLoad, JournalRecord, SlotId, StoreError,
    StoreIo, SLOT_A_PATH, SLOT_B_PATH, STORE_DIR,
};

#[derive(Default)]
struct FakeStore {
    dirs: BTreeMap<&'static str, ()>,
    files: BTreeMap<&'static str, Vec<u8>>,
    corrupt_readback: bool,
}

impl FakeStore {
    fn put_record(&mut self, path: &'static str, record: JournalRecord) {
        self.files.insert(path, record.encode().to_vec());
    }
}

impl StoreIo for FakeStore {
    fn ensure_dir(&mut self, path: &str) -> Result<(), StoreError> {
        if path == STORE_DIR {
            self.dirs.insert(STORE_DIR, ());
            Ok(())
        } else {
            Err(StoreError::Io)
        }
    }

    fn stat(&mut self, path: &str) -> Result<Option<u64>, StoreError> {
        Ok(self.files.get(path).map(|bytes| bytes.len() as u64))
    }

    fn read_file(&mut self, path: &str, _max_bytes: usize) -> Result<Option<Vec<u8>>, StoreError> {
        Ok(self.files.get(path).cloned())
    }

    fn write_file(&mut self, path: &str, content: &[u8]) -> Result<(), StoreError> {
        let Some(key) = [SLOT_A_PATH, SLOT_B_PATH]
            .into_iter()
            .find(|slot| *slot == path)
        else {
            return Err(StoreError::Io);
        };
        let mut bytes = content.to_vec();
        if self.corrupt_readback {
            bytes[0] ^= 0xFF;
        }
        self.files.insert(key, bytes);
        Ok(())
    }

    fn unlink(&mut self, path: &str) -> Result<(), StoreError> {
        self.files.remove(path);
        Ok(())
    }
}

#[test]
fn empty_store_loads_nonproduction_state() {
    let mut store = FakeStore::default();
    let state = load_for_tests(&mut store);
    assert_eq!(state.load, JournalLoad::Empty);
    assert!(state.active.is_none());
}

#[test]
fn one_valid_slot_loads_as_active() {
    let mut store = FakeStore::default();
    store.put_record(
        SLOT_A_PATH,
        JournalRecord::placeholder(SlotId::A, 4, 9, [0; 32]),
    );
    let state = load_for_tests(&mut store);
    assert_eq!(state.load, JournalLoad::Loaded);
    let active = state.active.unwrap();
    assert_eq!(active.slot, SlotId::A);
    assert_eq!(active.record.blob_revision, 4);
}

#[test]
fn torn_inactive_write_keeps_previous_slot_active() {
    let mut store = FakeStore::default();
    let mut state = load_for_tests(&mut store);
    persist_placeholder_for_tests(&mut state, &mut store, 1).unwrap();
    let active_before = state.active.clone().unwrap();
    store.corrupt_readback = true;
    let err = persist_placeholder_for_tests(&mut state, &mut store, 2).unwrap_err();
    assert_eq!(err, StoreError::ReadbackMismatch);
    assert_eq!(state.active.unwrap(), active_before);
}

#[test]
fn both_corrupted_slots_fall_back_to_empty() {
    let mut store = FakeStore::default();
    store.files.insert(SLOT_A_PATH, vec![1, 2, 3]);
    store.files.insert(SLOT_B_PATH, vec![4, 5, 6]);
    let state = load_for_tests(&mut store);
    assert_eq!(state.load, JournalLoad::Empty);
    assert!(state.active.is_none());
}

#[test]
fn stale_rollback_pair_fails_closed() {
    let mut store = FakeStore::default();
    let older = JournalRecord::placeholder(SlotId::A, 1, 0, [0; 32]);
    let newer = JournalRecord::placeholder(SlotId::B, 2, 0, [0; 32]);
    store.put_record(SLOT_A_PATH, older);
    store.put_record(SLOT_B_PATH, newer);
    let state = load_for_tests(&mut store);
    assert_eq!(state.load, JournalLoad::RollbackDetected);
    assert!(state.active.is_none());
}

#[test]
fn valid_slot_survives_corrupted_inactive_partner() {
    let mut store = FakeStore::default();
    let older = JournalRecord::placeholder(SlotId::A, 1, 0, [0; 32]);
    let newer = JournalRecord::placeholder(SlotId::B, 2, 0, older.digest());
    store.put_record(SLOT_A_PATH, older);
    store
        .files
        .insert(SLOT_B_PATH, vec![0; JournalRecord::ENCODED_LEN]);
    let state = load_for_tests(&mut store);
    assert_eq!(state.load, JournalLoad::Loaded);
    assert_eq!(state.active.unwrap().record.blob_revision, 1);
    store.put_record(SLOT_B_PATH, newer);
    let state = load_for_tests(&mut store);
    assert_eq!(state.active.unwrap().record.blob_revision, 2);
}
