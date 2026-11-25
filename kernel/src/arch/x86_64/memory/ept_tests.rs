//! Host tests for the nested-paging map boundary (C-2): a hostile page count
//! must never reach the walk, and a failed map must leave no partial mapping.

use super::*;
use crate::memory::cell_quota::{Account, Reservation};
use crate::memory::stage2::tests::FrameArena;

const TWO_MIB: u64 = 2 * 1024 * 1024;

#[test]
fn a_page_count_whose_span_wraps_is_refused_before_any_walk() {
    let arena = FrameArena::new(4);
    let mut table =
        NestedPageTable::new(NestedFormat::Ept, Reservation::kernel(Account::Guest)).expect("root");
    let used = arena.used();
    // 2^52 pages × 4 KiB wraps u64 to 0; usize::MAX is the syscall's
    // `size = usize::MAX` worst case.
    for n_pages in [1usize << 52, usize::MAX] {
        assert_eq!(
            table.map(0, 0x10_0000, n_pages, true),
            Err(EptMapError::Overflow)
        );
    }
    assert_eq!(arena.used(), used, "a refused map allocates nothing");
    assert_eq!(table.translate(0), None);
}

#[test]
fn a_map_larger_than_the_carve_is_refused() {
    let arena = FrameArena::new(8);
    let mut table =
        NestedPageTable::new(NestedFormat::Ept, Reservation::kernel(Account::Guest)).expect("root");
    let carve = table.carve_guest_ram(4).expect("carve");
    let used = arena.used();
    // `usize::MAX.div_ceil(PAGE_SIZE)` is what `MapGuestMemory` derives from
    // `size = usize::MAX`.
    for n_pages in [5, 1 << 52, usize::MAX.div_ceil(PAGE_SIZE), usize::MAX] {
        assert_eq!(
            table.map(0, carve, n_pages, true),
            Err(EptMapError::SasViolation)
        );
    }
    assert_eq!(arena.used(), used);
    assert_eq!(table.translate(0), None);
}

#[test]
fn a_map_that_runs_out_of_tables_writes_no_leaf() {
    // Root + 4-page carve + PDPT/PD/PT for the first 2 MiB: full afterwards.
    let arena = FrameArena::new(8);
    let mut table =
        NestedPageTable::new(NestedFormat::Ept, Reservation::kernel(Account::Guest)).expect("root");
    let carve = table.carve_guest_ram(4).expect("carve");
    table.map(0, carve, 1, true).expect("first page");
    assert_eq!(table.translate(0), Some(carve));

    // Two pages in the existing PT, two that need a new one.
    let start = TWO_MIB - 2 * PAGE_SIZE as u64;
    assert_eq!(
        table.map(start, carve, 4, true),
        Err(EptMapError::OutOfMemory)
    );
    for page in 0..4 {
        assert_eq!(table.translate(start + page * PAGE_SIZE as u64), None);
    }
    assert_eq!(table.translate(0), Some(carve), "earlier mapping untouched");
    assert_eq!(arena.used(), 8);
}

/// Phase 16 row 10: the table owns its frames as runs. Dropping it returns
/// every one — root, sub-tables, guest RAM — and a second carve is refused
/// rather than dropping RAM the table may already map.
#[test]
fn a_dropped_table_returns_every_frame_it_took() {
    let arena = FrameArena::new(8);
    let mut table =
        NestedPageTable::new(NestedFormat::Npt, Reservation::kernel(Account::Guest)).expect("root");
    let carve = table.carve_guest_ram(2).expect("carve");
    assert_eq!(table.carve_guest_ram(1), None, "a table carves once");
    table.map(0, carve, 2, true).expect("map");
    assert_eq!(arena.used(), 6, "root + PDPT + PD + PT + two RAM pages");
    drop(table);
    assert_eq!(arena.used(), 0);
}

/// Output addresses only: the leaves are never walked by a CPU here.
const HOST: u64 = 0x4000_0000;

/// Two whole 2 MiB ranges take one page-directory entry each and no page
/// table; the 3-page tail takes a page table. Unmapping one page inside a
/// large leaf splits it and keeps the other 511 translations.
#[test]
fn aligned_ranges_take_large_leaves_and_a_partial_unmap_splits_one() {
    let arena = FrameArena::new(8);
    let mut table =
        NestedPageTable::new(NestedFormat::Ept, Reservation::kernel(Account::Guest)).expect("root");
    table.allow_large_leaves();
    let used = arena.used();
    table.map(0, HOST, 1024 + 3, true).expect("map");
    assert_eq!(arena.used() - used, 3, "PDPT + PD + one PT for the tail");
    assert_eq!(table.translate(0x1234), Some(HOST + 0x1234));
    assert_eq!(
        table.translate(TWO_MIB + 0x5_4321),
        Some(HOST + TWO_MIB + 0x5_4321)
    );
    assert_eq!(
        table.translate(2 * TWO_MIB + 0x2fff),
        Some(HOST + 2 * TWO_MIB + 0x2fff)
    );
    assert_eq!(table.translate(2 * TWO_MIB + 0x3000), None, "past the tail");

    table.unmap(TWO_MIB + 0x3000, 1);
    assert_eq!(arena.used() - used, 4, "the split took a page table");
    assert_eq!(table.translate(TWO_MIB + 0x3000), None);
    assert_eq!(
        table.translate(TWO_MIB + 0x2fff),
        Some(HOST + TWO_MIB + 0x2fff)
    );
    assert_eq!(
        table.translate(TWO_MIB + 0x4000),
        Some(HOST + TWO_MIB + 0x4000)
    );
    assert_eq!(
        table.translate(0x1f_ffff),
        Some(HOST + 0x1f_ffff),
        "the other leaf"
    );
}

/// A host address off the 2 MiB grid, a range shorter than 2 MiB, or a table
/// that does not allow them: 4 KiB leaves.
#[test]
fn misaligned_or_short_ranges_and_plain_tables_take_page_tables() {
    let arena = FrameArena::new(16);
    for (large, gpa, host, pages) in [
        (true, 0, HOST + 0x1000, 512),
        (true, 0, HOST, 511),
        (true, 0x1000, HOST + 0x1000, 512),
        (false, 0, HOST, 512),
    ] {
        let mut table =
            NestedPageTable::new(NestedFormat::Ept, Reservation::kernel(Account::Guest))
                .expect("root");
        if large {
            table.allow_large_leaves();
        }
        let used = arena.used();
        table.map(gpa, host, pages, true).expect("map");
        assert!(arena.used() - used >= 3, "a page table was needed");
        let last = gpa + (pages as u64 - 1) * PAGE_SIZE as u64;
        assert_eq!(
            table.translate(last + 0x10),
            Some(host + (last - gpa) + 0x10)
        );
    }
}

/// Mapping a page into a range a large leaf covers splits the leaf; the new
/// page and every other translation hold, and the permission of the split
/// leaves is the large leaf's.
#[test]
fn a_page_mapped_over_a_large_leaf_splits_it() {
    let arena = FrameArena::new(8);
    let mut table =
        NestedPageTable::new(NestedFormat::Ept, Reservation::kernel(Account::Guest)).expect("root");
    table.allow_large_leaves();
    table.map(0, HOST, 512, false).expect("large leaf");
    let used = arena.used();
    table.map(0x7000, 0x9000_0000, 1, true).expect("one page");
    assert_eq!(arena.used() - used, 1, "the split's page table");
    assert_eq!(table.translate(0x7010), Some(0x9000_0010));
    assert_eq!(table.translate(0x6010), Some(HOST + 0x6010));
    assert_eq!(table.translate(0x8010), Some(HOST + 0x8010));
}
