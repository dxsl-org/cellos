extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;

use crate::lifecycle::{ActiveRelayGeneration, ProtectedRelayState, RelayLifecycle};
use crate::storage::{
    load_for_tests, persist_placeholder_for_tests, persist_relay_state_for_tests,
    protected_relay_state_for_tests, JournalKey, JournalLoad, JournalRecord, SlotId, StoreError,
    StoreIo, SLOT_A_PATH, SLOT_B_PATH, STORE_DIR,
};

const TEST_KEY: JournalKey = *b"cellos-kms-test-auth-key-v1-0000";
const WRONG_KEY: JournalKey = *b"cellos-kms-test-auth-key-v1-0001";

#[derive(Default)]
struct FakeStore {
    dirs: BTreeMap<&'static str, ()>,
    files: BTreeMap<&'static str, Vec<u8>>,
    corrupt_readback: bool,
}

impl FakeStore {
    fn put_record(&mut self, path: &'static str, record: JournalRecord) {
        self.put_record_with_key(path, &TEST_KEY, record);
    }

    fn put_record_with_key(&mut self, path: &'static str, key: &JournalKey, record: JournalRecord) {
        self.files.insert(path, record.encode(key).to_vec());
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
}

#[test]
fn empty_store_loads_nonproduction_state() {
    let mut store = FakeStore::default();
    let state = load_for_tests(&mut store, &TEST_KEY);
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
    let state = load_for_tests(&mut store, &TEST_KEY);
    assert_eq!(state.load, JournalLoad::Loaded);
    let active = state.active.unwrap();
    assert_eq!(active.slot, SlotId::A);
    assert_eq!(active.record.blob_revision, 4);
}

#[test]
fn torn_inactive_write_keeps_previous_slot_active() {
    let mut store = FakeStore::default();
    let mut state = load_for_tests(&mut store, &TEST_KEY);
    persist_placeholder_for_tests(&mut state, &mut store, &TEST_KEY, 1).unwrap();
    let active_before = state.active.clone().unwrap();
    store.corrupt_readback = true;
    let err = persist_placeholder_for_tests(&mut state, &mut store, &TEST_KEY, 2).unwrap_err();
    assert_eq!(err, StoreError::ReadbackMismatch);
    assert_eq!(state.active.unwrap(), active_before);
}

#[test]
fn both_corrupted_slots_fall_back_to_empty() {
    let mut store = FakeStore::default();
    store.files.insert(SLOT_A_PATH, vec![1, 2, 3]);
    store.files.insert(SLOT_B_PATH, vec![4, 5, 6]);
    let state = load_for_tests(&mut store, &TEST_KEY);
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
    let state = load_for_tests(&mut store, &TEST_KEY);
    assert_eq!(state.load, JournalLoad::RollbackDetected);
    assert!(state.active.is_none());
}

#[test]
fn valid_slot_survives_corrupted_inactive_partner() {
    let mut store = FakeStore::default();
    let older = JournalRecord::placeholder(SlotId::A, 1, 0, [0; 32]);
    let newer = JournalRecord::placeholder(SlotId::B, 2, 0, older.digest(&TEST_KEY));
    store.put_record(SLOT_A_PATH, older);
    store
        .files
        .insert(SLOT_B_PATH, vec![0; JournalRecord::ENCODED_LEN]);
    let state = load_for_tests(&mut store, &TEST_KEY);
    assert_eq!(state.load, JournalLoad::Loaded);
    assert_eq!(state.active.unwrap().record.blob_revision, 1);
    store.put_record(SLOT_B_PATH, newer);
    let state = load_for_tests(&mut store, &TEST_KEY);
    assert_eq!(state.active.unwrap().record.blob_revision, 2);
}

#[test]
fn wrong_key_rejects_otherwise_valid_slot() {
    let mut store = FakeStore::default();
    store.put_record(
        SLOT_A_PATH,
        JournalRecord::placeholder(SlotId::A, 1, 0, [0; 32]),
    );
    let state = load_for_tests(&mut store, &WRONG_KEY);
    assert_eq!(state.load, JournalLoad::Empty);
    assert!(state.active.is_none());
}

#[test]
fn keyed_corruption_rejects_slot_decode() {
    let mut store = FakeStore::default();
    let record = JournalRecord::placeholder(SlotId::A, 7, 3, [0; 32]);
    let mut encoded = record.encode(&TEST_KEY);
    encoded[40] ^= 0xAA;
    store.files.insert(SLOT_A_PATH, encoded.to_vec());
    let state = load_for_tests(&mut store, &TEST_KEY);
    assert_eq!(state.load, JournalLoad::Empty);
    assert!(state.active.is_none());
}

#[test]
fn known_provider_kind_decodes_without_expanding_unknown_values() {
    let mut store = FakeStore::default();
    let mut record = JournalRecord::placeholder(SlotId::A, 2, 5, [0; 32]);
    record.provider = types::kms::KmsProviderKind::DiceSealed;
    store.put_record(SLOT_A_PATH, record);
    let state = load_for_tests(&mut store, &TEST_KEY);
    assert_eq!(
        state.active.unwrap().record.provider,
        types::kms::KmsProviderKind::DiceSealed
    );

    let mut encoded = JournalRecord::placeholder(SlotId::A, 2, 5, [0; 32]).encode(&TEST_KEY);
    encoded[6] = 99;
    let auth = crate::storage::authenticator(&TEST_KEY, &encoded[..154]);
    encoded[154..].copy_from_slice(&auth);
    store.files.insert(SLOT_A_PATH, encoded.to_vec());
    let state = load_for_tests(&mut store, &TEST_KEY);
    assert_eq!(state.load, JournalLoad::Empty);
}

#[test]
fn protected_relay_lifecycle_round_trips_and_rejects_restart_regression() {
    let protected = ProtectedRelayState {
        active: Some(ActiveRelayGeneration {
            generation: 9,
            policy_epoch: 14,
            profile_digest: [0x6a; 32],
            revoked: false,
        }),
        authenticated_time_floor: 1_800_000_000,
        restart_epoch_floor: 41,
    };
    let mut store = FakeStore::default();
    let mut journal = load_for_tests(&mut store, &TEST_KEY);
    persist_relay_state_for_tests(&mut journal, &mut store, &TEST_KEY, protected).unwrap();
    let loaded = load_for_tests(&mut store, &TEST_KEY);
    assert_eq!(protected_relay_state_for_tests(&loaded), Some(protected));
    let recovered = RelayLifecycle::recover(42, protected).unwrap();
    assert_eq!(recovered.serving(), protected.active);
    assert!(matches!(
        RelayLifecycle::recover(41, protected),
        Err(types::kms::KmsErrorCode::PolicyEpochRegressed)
    ));

    // An authenticated slot with a torn lifecycle payload never recovers.
    let mut torn = loaded.active.unwrap().record;
    torn.sealed_leaf[16..48].fill(0);
    store.put_record(SLOT_A_PATH, torn);
    let torn = load_for_tests(&mut store, &TEST_KEY);
    assert!(protected_relay_state_for_tests(&torn).is_none());
}
