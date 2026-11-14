//! Host tests for the Stage-2 map boundary, plus the frame arena the nested
//! (EPT/NPT) table tests share.
//!
//! Page tables are real: the global frame allocator is pointed at a host heap
//! arena (the host lane's physical offset is 0, so a "PA" is the host address).

extern crate alloc;

use super::*;
use crate::memory::frame::{allocator_for_tests, FrameAllocator};
use crate::sync::SpinlockGuard;
use alloc::vec::Vec;

/// A bounded set of frames installed as the global allocator for one test.
///
/// Holds `TEST_STATE_LOCK` for its lifetime (the allocator is global) and
/// restores the previous allocator on drop. Declare page tables *after* the
/// arena so they are dropped — and return their frames — first.
pub(crate) struct FrameArena {
    previous: Option<FrameAllocator>,
    _backing: Vec<u8>,
    _serial: SpinlockGuard<'static, ()>,
}

impl FrameArena {
    /// `frames` usable frames, the first 8 KiB-aligned (a Stage-2 root pair).
    pub(crate) fn new(frames: usize) -> Self {
        let serial = crate::TEST_STATE_LOCK.lock();
        let backing = alloc::vec![0u8; (frames + 2) * PAGE_SIZE];
        let base = (backing.as_ptr() as usize).next_multiple_of(2 * PAGE_SIZE);
        let previous = crate::memory::frame::replace(Some(allocator_for_tests(&[(base, frames)])));
        Self {
            previous,
            _backing: backing,
            _serial: serial,
        }
    }

    pub(crate) fn used(&self) -> usize {
        crate::memory::frame::with(|allocator| allocator.used_frames()).unwrap_or(0)
    }
}

impl Drop for FrameArena {
    fn drop(&mut self) {
        crate::memory::frame::replace(self.previous.take());
    }
}

const IPA: u64 = 0x4000_0000;
/// Leaf output address only; never dereferenced, so any in-range value works.
const HPA: u64 = 0x8000_0000;
const TWO_MIB: u64 = 2 * 1024 * 1024;

/// The L3 descriptor for `ipa`, or 0 when any level is absent. Never allocates.
fn leaf(table: &Stage2Table, ipa: u64) -> u64 {
    // SAFETY: walks only valid table descriptors of a live table.
    unsafe {
        let l1e = *table.root_va.add(l1_idx(ipa));
        if l1e & DESC_VALID == 0 {
            return 0;
        }
        let l2e = *(phys_to_virt(desc_pa(l1e)) as *const u64).add(l2_idx(ipa));
        if l2e & DESC_VALID == 0 {
            return 0;
        }
        *(phys_to_virt(desc_pa(l2e)) as *const u64).add(l3_idx(ipa))
    }
}

#[test]
fn a_page_count_whose_span_wraps_is_refused_before_any_walk() {
    let arena = FrameArena::new(2);
    let mut table = Stage2Table::new().expect("root pair");
    let used = arena.used();
    // 2^52 pages × 4 KiB wraps u64 to 0; usize::MAX is the syscall's
    // `size = usize::MAX` worst case.
    for n_pages in [1usize << 52, usize::MAX] {
        assert_eq!(
            table.map(IPA, HPA, n_pages, true),
            Err(S2MapError::Overflow)
        );
        assert_eq!(
            table.map_mmio_passthrough(IPA, HPA, n_pages, true),
            Err(S2MapError::Overflow)
        );
    }
    assert_eq!(arena.used(), used, "a refused map allocates nothing");
    assert_eq!(leaf(&table, IPA), 0);
}

#[test]
fn a_map_larger_than_the_carve_is_refused() {
    let arena = FrameArena::new(4);
    let mut table = Stage2Table::new().expect("root pair");
    let carve = table.carve_guest_ram(2).expect("carve");
    let used = arena.used();
    for n_pages in [3usize, 1 << 52, usize::MAX] {
        assert_eq!(
            table.map(IPA, carve, n_pages, true),
            Err(S2MapError::SasViolation)
        );
    }
    assert_eq!(arena.used(), used);
    assert_eq!(leaf(&table, IPA), 0);
}

#[test]
fn a_map_that_runs_out_of_tables_writes_no_leaf() {
    // Root pair + one L2 + one L3: the arena is full after the first map.
    let arena = FrameArena::new(4);
    let mut table = Stage2Table::new().expect("root pair");
    table.map(IPA, HPA, 1, true).expect("first page");
    let existing = leaf(&table, IPA);
    assert_ne!(existing, 0);

    // Two pages in the existing L3, two that need a new one.
    let start = IPA + TWO_MIB - 2 * PAGE_SIZE as u64;
    assert_eq!(table.map(start, HPA, 4, true), Err(S2MapError::OutOfMemory));
    for page in 0..4 {
        assert_eq!(leaf(&table, start + page * PAGE_SIZE as u64), 0);
    }
    assert_eq!(leaf(&table, IPA), existing, "earlier mapping untouched");
    assert_eq!(arena.used(), 4);
}
