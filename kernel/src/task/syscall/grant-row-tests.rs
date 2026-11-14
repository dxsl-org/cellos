//! Review finding (Phase 15): a domain row's frames belong to exactly one
//! revoker. Revoke is idempotent, so concurrent revokers of one record all see
//! it succeed; only the one whose removal takes the row may free the frames.

use super::misc::{release_if_row_taken, remove_row_if};
use crate::sync::Spinlock;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use core::cell::Cell;

const KEY: usize = 0x5b10_0000;

fn carries(row: &Option<Arc<u32>>, record: &Arc<u32>) -> bool {
    row.as_ref()
        .is_some_and(|carried| Arc::ptr_eq(carried, record))
}

#[test]
fn second_revoker_of_one_record_does_not_take_the_row() {
    let record = Arc::new(1u32);
    let mut map = BTreeMap::from([(KEY, Some(Arc::clone(&record)))]);
    assert!(remove_row_if(Some(&mut map), KEY, |row| carries(
        row, &record
    )));
    // The racing revoker's removal must report that it owns nothing to free.
    assert!(!remove_row_if(Some(&mut map), KEY, |row| carries(
        row, &record
    )));
    assert!(map.is_empty());
}

#[test]
fn stale_revoker_leaves_a_new_row_at_the_same_key() {
    let stale = Arc::new(1u32);
    let fresh = Arc::new(1u32);
    // The first revoker freed the frames and a new grant reused the base (key).
    let mut map = BTreeMap::from([(KEY, Some(Arc::clone(&fresh)))]);
    assert!(!remove_row_if(Some(&mut map), KEY, |row| carries(
        row, &stale
    )));
    assert!(map.get(&KEY).is_some_and(|row| carries(row, &fresh)));
    // A SAS row (no record) at the key is not a domain row either.
    let mut sas: BTreeMap<usize, Option<Arc<u32>>> = BTreeMap::from([(KEY, None)]);
    assert!(!remove_row_if(Some(&mut sas), KEY, |row| carries(
        row, &stale
    )));
    assert_eq!(sas.len(), 1);
}

#[test]
fn absent_table_or_row_takes_nothing() {
    let record = Arc::new(1u32);
    assert!(!remove_row_if::<Option<Arc<u32>>>(
        None,
        KEY,
        |row| carries(row, &record)
    ));
    let mut empty: BTreeMap<usize, Option<Arc<u32>>> = BTreeMap::new();
    assert!(!remove_row_if(Some(&mut empty), KEY, |row| carries(
        row, &record
    )));
}

// The call-site shape every revoker uses (`GrantFree`, `GrantUnregister`, the
// owner's exit, the deferred sweep): the frames are released only by the
// revoker whose removal took the row.

fn table_with(row: Option<Arc<u32>>) -> Spinlock<Option<BTreeMap<usize, Option<Arc<u32>>>>> {
    Spinlock::new(Some(BTreeMap::from([(KEY, row)])))
}

#[test]
fn two_revokers_of_one_record_free_its_frames_once() {
    let record = Arc::new(1u32);
    let table = table_with(Some(Arc::clone(&record)));
    let frees = Cell::new(0);
    // e.g. `CapRevoke`'s reclaim, then a racing `GrantUnregister` whose revoke
    // of the same (idempotent) record also succeeded.
    for _ in 0..2 {
        release_if_row_taken(
            &table,
            KEY,
            |row| carries(row, &record),
            || frees.set(frees.get() + 1),
        );
    }
    assert_eq!(frees.get(), 1);
    assert!(table.lock().as_ref().is_some_and(BTreeMap::is_empty));
}

#[test]
fn late_revoker_neither_frees_nor_removes_a_reused_row() {
    let stale = Arc::new(1u32);
    let fresh = Arc::new(1u32);
    // The first revoker freed the frames and another cell's grant now holds the
    // same physical base, i.e. the same key.
    let table = table_with(Some(Arc::clone(&fresh)));
    let freed = Cell::new(false);
    let taken = release_if_row_taken(&table, KEY, |row| carries(row, &stale), || freed.set(true));
    assert!(!taken);
    assert!(!freed.get());
    assert!(table
        .lock()
        .as_ref()
        .and_then(|map| map.get(&KEY))
        .is_some_and(|row| carries(row, &fresh)));
}
