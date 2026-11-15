//! The grant table holds SHM regions too (Phase 16 row 13): a grant path must
//! never resolve, change or remove an SHM handle, and `ShmMap` must resolve
//! nothing but SHM handles.

use super::{Region, RegionOwner, RegionTable};
use crate::memory::frame_run::{FrameRun, KernelAlias, RunKind, RunOwner};
use types::CellId;

const SHM: usize = 0x5d10_0000;
const GRANT: usize = 0x5d20_0000;
const OWNER_TID: usize = 54_001;

/// A run over synthetic frames: discarded at the end, never freed.
fn run(base: usize) -> FrameRun {
    FrameRun::mapped(
        base,
        1,
        RunOwner::Kernel,
        RunKind::Leaf,
        KernelAlias::Intact,
    )
}

fn table() -> (RegionTable, RegionOwner) {
    let shm_owner = RegionOwner::incarnation(7, 1);
    let mut table = RegionTable::new();
    assert!(table
        .insert(SHM, Region::shm(shm_owner, run(SHM), 4096))
        .is_none());
    let grant_owner = RegionOwner::task(OWNER_TID, CellId(8), 1);
    assert!(table
        .insert(GRANT, Region::grant(grant_owner, run(GRANT), 4096, None))
        .is_none());
    (table, shm_owner)
}

fn discard(table: RegionTable) {
    for (_, region) in table.rows {
        region.discard_synthetic();
    }
}

#[test]
fn the_grant_view_never_sees_an_shm_region() {
    let (mut table, _) = table();
    assert!(table.get(&SHM).is_none());
    assert!(table.get_mut(&SHM).is_none());
    // A grant removal by an SHM handle takes nothing and leaves the row.
    assert!(table.remove(&SHM).is_none());
    assert!(table.shm(&SHM).is_some());
    let grants: alloc::vec::Vec<usize> = table.iter().map(|(&id, _)| id).collect();
    assert_eq!(grants, [GRANT]);
    assert_eq!(table.values_mut().count(), 1);
    // The key is still in use for an insert-if-free check.
    assert!(table.occupied(&SHM));
    discard(table);
}

#[test]
fn the_shm_view_never_sees_a_grant() {
    let (mut table, shm_owner) = table();
    assert!(table.shm(&GRANT).is_none());
    assert!(table.shm_mut(&GRANT).is_none());
    // The SHM view takes no grant, whoever owns it.
    assert!(table
        .take_shm_of(RegionOwner::task(OWNER_TID, CellId(8), 1))
        .is_empty());
    let taken = table.take_shm_of(shm_owner);
    assert_eq!(taken.len(), 1);
    assert_eq!(taken[0].base(), SHM);
    taken.into_iter().for_each(Region::discard_synthetic);
    assert!(table.get(&GRANT).is_some());
    assert!(!table.occupied(&SHM));
    discard(table);
}

/// An occupied id refuses the insert, whatever kind either row is: the row
/// already there keeps its run, and the refused region comes back whole.
#[test]
fn an_insert_never_replaces_a_row() {
    let (mut table, shm_owner) = table();
    let other = RegionOwner::task(OWNER_TID + 1, CellId(9), 1);
    for (id, region) in [
        (SHM, Region::grant(other, run(SHM), 8192, None)),
        (GRANT, Region::shm(other, run(GRANT), 8192)),
    ] {
        let refused = table.insert(id, region).expect("occupied id");
        assert_eq!((refused.owner, refused.size), (other, 8192));
        refused.discard_synthetic();
    }
    assert_eq!(
        table.shm(&SHM).map(|r| (r.owner, r.size)),
        Some((shm_owner, 4096))
    );
    assert_eq!(table.get(&GRANT).map(|r| r.size), Some(4096));
    discard(table);
}
