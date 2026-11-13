//! Host tests for the nested-paging map boundary (C-2): a hostile page count
//! must never reach the walk, and a failed map must leave no partial mapping.

use super::*;
use crate::memory::stage2::tests::FrameArena;

const TWO_MIB: u64 = 2 * 1024 * 1024;

#[test]
fn a_page_count_whose_span_wraps_is_refused_before_any_walk() {
    let arena = FrameArena::new(4);
    let mut table = NestedPageTable::new(NestedFormat::Ept).expect("root");
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
    let mut table = NestedPageTable::new(NestedFormat::Ept).expect("root");
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
    let mut table = NestedPageTable::new(NestedFormat::Ept).expect("root");
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
