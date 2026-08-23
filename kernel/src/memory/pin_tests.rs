//! Unit tests for the async pinning registry.
//!
//! The registry is a single global table, so every test works on a base address
//! range and owner id of its own and acknowledges what it pinned before
//! returning. Ranges are 1 MiB apart so no two tests can overlap.

use super::*;

/// Distinct 1 MiB test arena per case, well clear of any real frame.
const fn arena(n: usize) -> usize {
    0x4000_0000 + n * 0x10_0000
}

#[test]
fn refuses_an_empty_or_overflowing_range() {
    assert_eq!(pin(arena(0), 0, 900), Err(PinError::InvalidRange));
    assert_eq!(pin(usize::MAX - 8, 64, 900), Err(PinError::InvalidRange));
}

#[test]
fn a_pinned_region_reports_its_holder() {
    let base = arena(1);
    assert_eq!(pin(base, PAGE_SIZE, 901), Ok(()));
    let held = holder_of(base, PAGE_SIZE).expect("region must report as pinned");
    assert_eq!(held.owner, 901);
    assert_eq!(held.pages, 1);
    assert!(!held.quarantined);
    acknowledge(901);
    assert!(holder_of(base, PAGE_SIZE).is_none());
}

#[test]
fn a_partial_overlap_still_counts_as_pinned() {
    let base = arena(2);
    // Pin the middle page of a three-page span, then ask about the whole span:
    // a teardown of the enclosing grant must not be allowed to proceed.
    assert_eq!(pin(base + PAGE_SIZE, PAGE_SIZE, 902), Ok(()));
    assert!(holder_of(base, 3 * PAGE_SIZE).is_some());
    assert!(holder_of(base + 2 * PAGE_SIZE, PAGE_SIZE).is_none());
    acknowledge(902);
}

#[test]
fn an_unaligned_range_pins_every_page_it_touches() {
    let base = arena(3);
    assert_eq!(pin(base + 8, 16, 903), Ok(()));
    let held = holder_of(base, PAGE_SIZE).expect("head page must be covered");
    assert_eq!(held.base, base);
    assert_eq!(held.pages, 1);
    acknowledge(903);
}

#[test]
fn repinning_the_same_range_reuses_the_slot() {
    let base = arena(4);
    assert_eq!(pin(base, PAGE_SIZE, 904), Ok(()));
    assert_eq!(pin(base, PAGE_SIZE, 904), Ok(()));
    assert_eq!(holder_of(base, PAGE_SIZE).map(|h| h.holds), Some(2));
    // One acknowledgement releases the region regardless of hold count.
    assert!(acknowledge(904).is_empty());
    assert!(holder_of(base, PAGE_SIZE).is_none());
}

#[test]
fn a_single_owner_cannot_exhaust_the_table() {
    let base = arena(5);
    for i in 0..MAX_PINS_PER_TASK {
        assert_eq!(pin(base + i * PAGE_SIZE, PAGE_SIZE, 905), Ok(()));
    }
    assert_eq!(
        pin(base + MAX_PINS_PER_TASK * PAGE_SIZE, PAGE_SIZE, 905),
        Err(PinError::TaskLimit)
    );
    acknowledge(905);
    assert!(holder_of(base, PAGE_SIZE).is_none());
}

#[test]
fn death_quarantines_rather_than_releases() {
    let base = arena(6);
    assert_eq!(pin(base, 2 * PAGE_SIZE, 906), Ok(()));
    assert_eq!(quarantine_task(906), 1);
    // The reaper hands the frames to the quarantine instead of the allocator.
    let before = quarantined_pages();
    assert!(withhold_frames(base, 2, 906));
    assert_eq!(quarantined_pages(), before + 2);
    // Still refused to a teardown request: the frames are not the owner's to
    // release, and they are not the allocator's to hand out.
    let held = holder_of(base, PAGE_SIZE).expect("quarantined region stays pinned");
    assert!(held.quarantined);

    assert_eq!(acknowledge(906), alloc::vec![(base, 2)]);
    assert!(holder_of(base, PAGE_SIZE).is_none());
    assert_eq!(quarantined_pages(), before);
}

#[test]
fn an_acknowledgement_before_death_leaves_nothing_to_quarantine() {
    let base = arena(7);
    assert_eq!(pin(base, PAGE_SIZE, 907), Ok(()));
    assert!(acknowledge(907).is_empty());
    assert_eq!(quarantine_task(907), 0);
    // Nothing pinned means the reaper frees the frames on its own.
    assert!(holder_of(base, PAGE_SIZE).is_none());
}

#[test]
fn only_frames_the_reaper_withheld_are_ever_released() {
    // A range pinned but never handed to the quarantine — an MMIO window a
    // driver authorised for DMA, say — must not come back as frames to free.
    let base = arena(8);
    assert_eq!(pin(base, PAGE_SIZE, 908), Ok(()));
    assert_eq!(quarantine_task(908), 1);
    assert!(acknowledge(908).is_empty());
    assert!(holder_of(base, PAGE_SIZE).is_none());
}

#[test]
fn quarantine_and_acknowledgement_are_per_owner() {
    let mine = arena(9);
    let theirs = arena(10);
    assert_eq!(pin(mine, PAGE_SIZE, 909), Ok(()));
    assert_eq!(pin(theirs, PAGE_SIZE, 910), Ok(()));
    assert_eq!(quarantine_task(909), 1);
    assert!(withhold_frames(mine, 1, 909));
    assert!(withhold_frames(theirs, 1, 910));
    assert_eq!(acknowledge(909), alloc::vec![(mine, 1)]);
    assert!(holder_of(theirs, PAGE_SIZE).is_some());
    assert_eq!(acknowledge(910), alloc::vec![(theirs, 1)]);
}

#[test]
fn frames_are_charged_to_the_pin_holder_not_the_dead_owner() {
    // A driver cell authorises DMA against another cell's buffer. When that
    // other cell dies, the driver's acknowledgement is the one that counts.
    let base = arena(11);
    let driver = 911;
    assert_eq!(pin(base, PAGE_SIZE, driver), Ok(()));
    let held = holder_of(base, PAGE_SIZE).expect("driver holds the pin");
    assert_eq!(held.owner, driver);
    assert!(withhold_frames(base, 1, held.owner));
    // The dead buffer owner acknowledging nothing releases nothing.
    assert!(acknowledge(912).is_empty());
    assert_eq!(acknowledge(driver), alloc::vec![(base, 1)]);
}

#[test]
fn vfs_release_is_exact_to_the_request_and_target() {
    let base = arena(12);
    assert_eq!(pin_vfs_lease(base, PAGE_SIZE, 912, 301, 7, 1), Ok(()));
    assert_eq!(release_vfs_lease(301, 913, 1), alloc::vec![]);
    assert!(holder_of(base, PAGE_SIZE).is_some());
    assert_eq!(release_vfs_lease(301, 912, 2), alloc::vec![]);
    assert!(holder_of(base, PAGE_SIZE).is_some());
    assert_eq!(release_vfs_lease(301, 912, 1), alloc::vec![]);
    assert!(holder_of(base, PAGE_SIZE).is_none());
}

#[test]
fn owner_death_pending_revokes_until_the_matching_vfs_release() {
    let base = arena(13);
    assert_eq!(pin_vfs_lease(base, PAGE_SIZE, 913, 302, 8, 9), Ok(()));
    assert!(mark_vfs_lease_pending_revoke(302, 913, 9));
    assert!(vfs_lease_pending_revoke(302, 913, 9));
    let before = quarantined_pages();
    assert_eq!(withhold_pinned_frames(base, 1), FrameTransfer::Withheld);
    assert_eq!(quarantined_pages(), before + 1);
    let held = holder_of(base, PAGE_SIZE).expect("lease stays tracked while quarantined");
    assert!(held.quarantined && held.pending_revoke);
    assert_eq!(release_vfs_lease(302, 913, 8), alloc::vec![]);
    assert_eq!(quarantined_pages(), before + 1);
    assert_eq!(release_vfs_lease(302, 913, 9), alloc::vec![(base, 1)]);
    assert_eq!(quarantined_pages(), before);
}

#[test]
fn smp_vfs_holder_release_before_reaper_transfer_leaves_no_orphan() {
    // This is the release-wins side of the SMP interleaving. Owner death has
    // pending-revoked the exact lease, then the holder completes before the
    // reaper's transfer transaction linearizes. The reaper must free normally,
    // not create a quarantine entry keyed to the vanished lease.
    let base = arena(18);
    let before = quarantined_pages();
    assert_eq!(pin_vfs_lease(base, PAGE_SIZE, 918, 306, 13, 3), Ok(()));
    assert!(mark_vfs_lease_pending_revoke(306, 918, 3));
    assert_eq!(release_vfs_lease(306, 918, 3), alloc::vec![]);

    assert!(!withhold_vfs_frames(base, 1, 306, 918, 3));
    assert_eq!(quarantined_pages(), before);

    assert_eq!(withhold_pinned_frames(base, 1), FrameTransfer::Free);
    assert_eq!(quarantined_pages(), before);
    assert!(holder_of(base, PAGE_SIZE).is_none());
}

#[test]
fn dead_vfs_holder_releases_every_quarantined_lease_it_held() {
    let first = arena(14);
    let second = arena(15);
    assert_eq!(pin_vfs_lease(first, PAGE_SIZE, 914, 303, 9, 1), Ok(()));
    assert_eq!(pin_vfs_lease(second, PAGE_SIZE, 915, 303, 10, 2), Ok(()));
    assert!(withhold_vfs_frames(first, 1, 303, 914, 1));
    assert!(withhold_vfs_frames(second, 1, 303, 915, 2));
    assert_eq!(
        release_vfs_holder_leases(303),
        alloc::vec![(first, 1), (second, 1)]
    );
    assert!(holder_of(first, PAGE_SIZE).is_none());
    assert!(holder_of(second, PAGE_SIZE).is_none());
}

#[test]
fn vfs_owner_query_filters_by_overlapping_owner() {
    let first = arena(16);
    let second = arena(17);
    assert_eq!(pin_vfs_lease(first, PAGE_SIZE, 916, 304, 11, 1), Ok(()));
    assert_eq!(pin_vfs_lease(second, PAGE_SIZE, 917, 305, 12, 2), Ok(()));
    let held = vfs_holder_of_owner(first, PAGE_SIZE, 916).expect("owner lease must match");
    assert_eq!(held.owner, 916);
    assert_eq!(held.holder_tid, 304);
    assert!(vfs_holder_of_owner(first, PAGE_SIZE, 917).is_none());
    assert!(vfs_holder_of_owner(second, PAGE_SIZE, 916).is_none());
    assert_eq!(release_vfs_lease(304, 916, 1), alloc::vec![]);
    assert_eq!(release_vfs_lease(305, 917, 2), alloc::vec![]);
}
