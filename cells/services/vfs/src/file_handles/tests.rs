use super::*;
use types::CellId;

const CELL_A: Caller = Caller::principal(CellId(11), 1);
const CELL_B: Caller = Caller::principal(CellId(22), 1);
const CELL_B_RESPAWNED: Caller = Caller::principal(CellId(22), 2);

#[test]
fn close_rejects_a_non_owner_and_keeps_the_entry() {
    let mut table = FileHandleTable::new();
    let handle = table.insert(CELL_B, "/tmp/a", 7).expect("file handle");
    assert!(!table.close(CELL_A, handle));
    assert!(table.contains(handle));
    assert!(table.close(CELL_B, handle));
}

#[test]
fn begin_and_finish_sync_read_restore_the_open_state() {
    let mut table = FileHandleTable::new();
    let handle = table.insert(CELL_A, "/tmp/a", 3).expect("file handle");
    assert_eq!(
        table.begin_sync_read(CELL_A, handle).as_deref(),
        Some("/tmp/a")
    );
    assert!(table.finish_sync_read(CELL_A, handle));
    assert_eq!(
        table.begin_sync_read(CELL_A, handle).as_deref(),
        Some("/tmp/a")
    );
}

#[test]
fn path_lookup_hides_unknown_and_wrong_owner_handles() {
    let mut table = FileHandleTable::new();
    let handle = table.insert(CELL_B, "/tmp/b", 7).expect("file handle");
    assert_eq!(table.path_of(CELL_B, handle), Some("/tmp/b"));
    assert_eq!(table.path_of(CELL_A, handle), None);
    assert_eq!(table.path_of(CELL_A, ViVfsFileHandle(u64::MAX)), None);
    assert!(table.close(CELL_B, handle));
    assert_eq!(table.path_of(CELL_B, handle), None);
}

#[test]
fn parent_dir_revocation_reaps_cross_owner_entries() {
    let mut table = FileHandleTable::new();
    let keep = table.insert(CELL_A, "/tmp/a", 1).expect("keep");
    let drop_a = table.insert(CELL_A, "/tmp/b", 7).expect("drop_a");
    let drop_b = table.insert(CELL_B, "/tmp/c", 8).expect("drop_b");
    assert_eq!(table.revoke_by_parent_dirs(&[7, 8]), 2);
    assert!(table.contains(keep));
    assert!(!table.contains(drop_a));
    assert!(!table.contains(drop_b));
}

#[test]
fn purge_owner_is_exact_to_the_generation() {
    let mut table = FileHandleTable::new();
    let old = table.insert(CELL_B, "/tmp/old", 1).expect("old");
    let new = table.insert(CELL_B_RESPAWNED, "/tmp/new", 2).expect("new");
    assert_eq!(table.purge_owner(CELL_B), 1);
    assert!(!table.contains(old));
    assert!(table.contains(new));
}

#[test]
fn handles_are_not_reused_after_close() {
    let mut table = FileHandleTable::new();
    let first = table.insert(CELL_A, "/tmp/a", 1).expect("first");
    assert!(table.close(CELL_A, first));
    let second = table.insert(CELL_A, "/tmp/b", 1).expect("second");
    assert_ne!(first, second);
}

#[test]
fn per_owner_quota_is_32_handles() {
    let mut table = FileHandleTable::new();
    for i in 0..MAX_FILE_HANDLES_PER_CALLER {
        let path = alloc::format!("/tmp/{i}");
        table.insert(CELL_A, &path, 9).expect("within quota");
    }
    assert_eq!(table.held_by(CELL_A), MAX_FILE_HANDLES_PER_CALLER);
    assert_eq!(
        table.insert(CELL_A, "/tmp/overflow", 9),
        Err(FileHandleError::TooManyHandles)
    );
    assert_eq!(table.held_by(CELL_B), 0);
}

#[test]
fn monotonic_ids_fail_closed_on_exhaustion() {
    let mut table = FileHandleTable::new();
    table.set_next_for_test(u64::MAX, false);
    let last = table.insert(CELL_A, "/tmp/a", 1).expect("last handle");
    assert_eq!(last, ViVfsFileHandle(u64::MAX));
    assert_eq!(
        table.insert(CELL_A, "/tmp/b", 1),
        Err(FileHandleError::Exhausted)
    );
}

#[test]
fn file_handle_table_leased_entry_cleans_up_on_close_and_revocation() {
    let mut table = FileHandleTable::new();
    let ledger = crate::namespace::NamespaceLedger::new();
    let key = crate::namespace::NamespaceKey::parse("/srv/fh").expect("key");
    let lease = ledger.acquire_service_handle(&key).expect("lease");
    assert_eq!(ledger.entry_count(), 1);
    let handle = table.insert_leased(CELL_A, "/srv/fh", 5, Some(lease)).expect("handle");
    assert!(table.close(CELL_A, handle));
    assert_eq!(ledger.entry_count(), 0);

    let lease2 = ledger.acquire_service_handle(&key).expect("lease2");
    assert_eq!(ledger.entry_count(), 1);
    let _h2 = table.insert_leased(CELL_A, "/srv/fh", 5, Some(lease2)).expect("h2");
    assert_eq!(table.revoke_by_parent_dirs(&[5]), 1);
    assert_eq!(ledger.entry_count(), 0);
}
