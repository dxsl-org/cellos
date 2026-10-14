//! Actual private physical ledger behavior; no kernel allocator/HAL execution.
use super::*;

fn allocate(frame: usize, epoch: u64) {
    let write = p01_retained::physical_write().unwrap();
    p01_retained::physical_allocated(&write, frame, epoch);
}
fn free(frame: usize, epoch: u64) {
    let write = p01_retained::physical_write().unwrap();
    p01_retained::physical_freed(&write, frame, epoch);
}

#[test]
fn physical_shared_table_detach_is_not_bitmap_free() {
    let _serial = reset();
    allocate(4096, 2);
    p01_retained::physical_table_created(4096, 8192, 0x400000);
    p01_retained::physical_table_touched(4096, 8192, false);
    p01_retained::physical_table_touched(4096, 8192, false);
    p01_retained::physical_table_touched(4096, 8192, true);
    p01_retained::physical_table_detached(4096);
    let detached = p01_retained::copy_physical_entry(0).unwrap();
    assert_eq!(&detached[5..11], &[1, 8192, 0x400000, 2, 2, 1]);
    assert_eq!(detached[3], 0);
    free(4096, 3);
    let freed = p01_retained::copy_physical_entry(0).unwrap();
    assert_eq!(freed[3], 3);
    assert_eq!(&freed[..3], &detached[..3]);
    // Reuse has a fresh incarnation and never destroys background-release history.
    allocate(4096, 4);
    let reused = p01_retained::copy_physical_entry(1).unwrap();
    assert_ne!(reused[0], freed[0]);
    assert_eq!(&reused[1..4], &[4096, 4, 0]);
    assert_eq!(p01_retained::copy_physical_entry(0).unwrap()[3], 3);
}

#[test]
fn physical_existing_table_does_not_fabricate_create_provenance() {
    let _serial = reset();
    allocate(4096, 2);
    p01_retained::physical_table_touched(4096, 8192, false);
    let table = p01_retained::copy_physical_entry(0).unwrap();
    assert_eq!(&table[5..10], &[5, 8192, 0, 1, 1]);
    p01_retained::physical_table_touched(4096, 12288, false);
    assert_eq!(p01_retained::copy_physical_entry(0).unwrap()[6], 8192);
    let before = p01_retained::physical_snapshot_before();
    assert_eq!(p01_retained::physical_snapshot_after(before).status, 4);
}

#[test]
fn physical_post_load_failure_inventory_preserves_each_allocation() {
    let _serial = reset();
    for edge in 2..=4 {
        let frame = edge as usize * 4096;
        allocate(frame, edge);
        p01_retained::physical_prepare_failed(frame, 0x400000 + frame, 0x400000, edge);
        let entry = p01_retained::copy_physical_entry((edge - 2) as usize).unwrap();
        assert_eq!(&entry[1..4], &[frame as u64, edge, 0]);
        assert_eq!(entry[5], edge);
        assert_eq!(entry[7], (0x400000 + frame) as u64);
        assert_eq!(entry[11], 0x400000);
        assert_eq!(entry[6], 0); // No invented selected Owner or root.
    }
    // The existing retained union is deliberately unaffected by these witnesses.
    assert_eq!(p01_retained::metric().value, 0);
}

#[test]
fn physical_bitmap_writer_invalidates_overlapping_snapshot() {
    let _serial = reset();
    let before = p01_retained::physical_snapshot_before();
    allocate(4096, 2);
    assert_ne!(p01_retained::physical_snapshot_after(before).status, 1);
    let coherent = p01_retained::physical_snapshot_before();
    assert_eq!(p01_retained::physical_snapshot_after(coherent).status, 1);
    p01_measurement::ADMITTED.store(false, SeqCst);
    assert!(p01_retained::physical_write().is_none());
    assert_eq!(p01_measurement::FLAGS.load(SeqCst), 0);
}

#[test]
fn physical_duplicate_live_frame_and_unidentified_release_fail_closed() {
    let _serial = reset();
    allocate(4096, 2);
    allocate(4096, 3);
    assert!(p01_retained::copy_physical_entry(1).is_none());
    free(8192, 4);
    let before = p01_retained::physical_snapshot_before();
    assert_eq!(p01_retained::physical_snapshot_after(before).status, 4);
}

#[test]
fn physical_capacity_is_observation_ceiling_not_admission() {
    let _serial = reset();
    for index in 0..=p01_retained::physical_capacity() {
        allocate(index * 4096, index as u64 + 2);
    }
    assert!(p01_retained::copy_physical_entry(p01_retained::physical_capacity() - 1).is_some());
    let before = p01_retained::physical_snapshot_before();
    assert_eq!(p01_retained::physical_snapshot_after(before).status, 4);
}

#[test]
fn physical_free_without_detach_and_closed_ambient_failure_do_not_invent_edges() {
    let _serial = reset();
    allocate(4096, 2);
    p01_retained::physical_table_created(4096, 8192, 0x400000);
    free(4096, 3);
    let entry = p01_retained::copy_physical_entry(0).unwrap();
    assert_eq!((entry[3], entry[8]), (3, 1));
    let before = p01_retained::physical_snapshot_before();
    assert_eq!(p01_retained::physical_snapshot_after(before).status, 4);
    p01_measurement::ADMITTED.store(false, SeqCst);
    p01_retained::physical_prepare_failed(8192, 0x800000, 0x800000, 2);
    assert_eq!(p01_measurement::FLAGS.load(SeqCst), 0);
}

#[test]
fn physical_prearm_census_release_and_reuse_preserve_unknown_birth() {
    let _serial = reset();
    p01_measurement::ADMITTED.store(false, SeqCst);
    assert!(p01_retained::physical_write().is_none());
    // This is an exact used bit under the source lock, not a made-up allocation.
    p01_retained::physical_preexisting(4096, 1);
    let baseline = p01_retained::copy_physical_entry(0).unwrap();
    assert_eq!(&baseline[1..6], &[4096, 0, 0, 0, 6]);
    assert_eq!(baseline[11], 1);
    p01_measurement::ADMITTED.store(true, SeqCst);
    free(4096, 2);
    let released = p01_retained::copy_physical_entry(0).unwrap();
    assert_eq!(&released[1..6], &[4096, 0, 2, 0, 6]);
    assert_eq!(released[11], 1);
    allocate(4096, 3);
    let fresh = p01_retained::copy_physical_entry(1).unwrap();
    assert_ne!(fresh[0], baseline[0]);
    assert_eq!(&fresh[1..4], &[4096, 3, 0]);
    let before = p01_retained::physical_snapshot_before();
    assert_eq!(p01_retained::physical_snapshot_after(before).status, 1);
}

#[test]
fn physical_prearm_table_census_does_not_invent_creation_or_birth() {
    let _serial = reset();
    p01_measurement::ADMITTED.store(false, SeqCst);
    p01_retained::physical_preexisting(4096, 8);
    p01_measurement::ADMITTED.store(true, SeqCst);
    p01_retained::physical_table_touched(4096, 8192, false);
    let touched = p01_retained::copy_physical_entry(0).unwrap();
    assert_eq!(touched[2], 0);
    assert_eq!(&touched[4..12], &[0, 7, 8192, 0, 1, 1, 0, 8]);
    p01_retained::physical_table_detached(4096);
    free(4096, 9);
    let freed = p01_retained::copy_physical_entry(0).unwrap();
    assert_eq!((freed[2], freed[3], freed[8], freed[11]), (0, 9, 2, 8));
    let before = p01_retained::physical_snapshot_before();
    assert_eq!(p01_retained::physical_snapshot_after(before).status, 1);
}

#[test]
fn physical_prearm_census_impossible_release_epoch_fails_closed() {
    let _serial = reset();
    p01_retained::physical_preexisting(4096, 8);
    free(4096, 8);
    assert_eq!(p01_retained::copy_physical_entry(0).unwrap()[3], 8);
    let before = p01_retained::physical_snapshot_before();
    assert_eq!(p01_retained::physical_snapshot_after(before).status, 4);
}

#[test]
fn physical_post_load_failure_free_keeps_failure_witness() {
    let _serial = reset();
    for edge in 2..=4 {
        let frame = edge as usize * 4096;
        allocate(frame, edge * 2);
        p01_retained::physical_prepare_failed(frame, 0x400000 + frame, 0x400000, edge);
        free(frame, edge * 2 + 1);
        let entry = p01_retained::copy_physical_entry((edge - 2) as usize).unwrap();
        assert_eq!(&entry[1..4], &[frame as u64, edge * 2, edge * 2 + 1]);
        assert_eq!((entry[5], entry[7], entry[11]),
            (edge, (0x400000 + frame) as u64, 0x400000));
    }
    let before = p01_retained::physical_snapshot_before();
    assert_eq!(p01_retained::physical_snapshot_after(before).status, 1);
}
