//! Review finding (Phase 15): a domain row's frames belong to exactly one
//! revoker. Revoke is idempotent, so concurrent revokers of one record all see
//! it succeed; only the one whose removal takes the row may free the frames,
//! and the row hands them over (its region owns them).

use super::grant_run;
use super::misc::{release_if_row_taken, remove_row_if};
use crate::memory::region::{Region, RegionOwner, RegionTable};
use crate::sync::Spinlock;
use core::cell::Cell;
use types::CellId;

const KEY: usize = 0x5b10_0000;
/// Stand-ins for two lifecycle records: the row "carries" the record whose
/// owner tid it names.
const RECORD: usize = 55_001;
const OTHER: usize = 55_002;

fn row(record: usize) -> Region {
    Region::grant(
        RegionOwner::task(record, CellId(9), 1),
        grant_run(KEY, 1),
        4096,
        None,
    )
}

fn carries(row: &Region, record: usize) -> bool {
    row.owner.tid == record
}

fn table_with(record: usize) -> RegionTable {
    let mut table = RegionTable::new();
    assert!(table.insert(KEY, row(record)).is_none());
    table
}

/// Drop a test table's rows; their runs name synthetic frames.
fn discard(mut table: RegionTable) {
    if let Some(row) = table.remove(&KEY) {
        row.discard_synthetic();
    }
}

#[test]
fn second_revoker_of_one_record_does_not_take_the_row() {
    let mut map = table_with(RECORD);
    let taken = remove_row_if(Some(&mut map), KEY, |row| carries(row, RECORD));
    assert!(taken.as_ref().is_some_and(|row| row.base() == KEY));
    taken.map(Region::discard_synthetic);
    // The racing revoker's removal must report that it owns nothing to free.
    assert!(remove_row_if(Some(&mut map), KEY, |row| carries(row, RECORD)).is_none());
    assert!(map.get(&KEY).is_none());
}

#[test]
fn stale_revoker_leaves_a_new_row_at_the_same_key() {
    // The first revoker freed the frames and a new grant reused the base (key).
    let mut map = table_with(OTHER);
    assert!(remove_row_if(Some(&mut map), KEY, |row| carries(row, RECORD)).is_none());
    assert!(map.get(&KEY).is_some_and(|row| carries(row, OTHER)));
    discard(map);
}

#[test]
fn absent_table_or_row_takes_nothing() {
    assert!(remove_row_if(None, KEY, |row| carries(row, RECORD)).is_none());
    let mut empty = RegionTable::new();
    assert!(remove_row_if(Some(&mut empty), KEY, |row| carries(row, RECORD)).is_none());
}

// The call-site shape every revoker uses (`GrantFree`, `GrantUnregister`, the
// owner's exit, the deferred sweep): the frames are released only by the
// revoker whose removal took the row.

#[test]
fn two_revokers_of_one_record_free_its_frames_once() {
    let table = Spinlock::new(Some(table_with(RECORD)));
    let frees = Cell::new(0);
    // e.g. `CapRevoke`'s reclaim, then a racing `GrantUnregister` whose revoke
    // of the same (idempotent) record also succeeded.
    for _ in 0..2 {
        release_if_row_taken(
            &table,
            KEY,
            |row| carries(row, RECORD),
            |row| {
                frees.set(frees.get() + 1);
                row.discard_synthetic();
            },
        );
    }
    assert_eq!(frees.get(), 1);
    assert!(table
        .lock()
        .as_ref()
        .is_some_and(|map| map.values().next().is_none()));
}

#[test]
fn late_revoker_neither_frees_nor_removes_a_reused_row() {
    // The first revoker freed the frames and another cell's grant now holds the
    // same physical base, i.e. the same key.
    let table = Spinlock::new(Some(table_with(OTHER)));
    let freed = Cell::new(false);
    let taken = release_if_row_taken(
        &table,
        KEY,
        |row| carries(row, RECORD),
        |row| {
            freed.set(true);
            row.discard_synthetic();
        },
    );
    assert!(!taken);
    assert!(!freed.get());
    let left = table.lock().take();
    assert!(left
        .as_ref()
        .and_then(|map| map.get(&KEY))
        .is_some_and(|row| carries(row, OTHER)));
    left.map(discard);
}
