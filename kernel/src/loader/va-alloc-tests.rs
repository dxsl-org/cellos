//! Host tests for slot ownership: a fixed-VA image can claim only a slot no
//! other spawn holds, claimed slots never come back out of `alloc`, and random
//! start indices change which free slot is taken, never whether it is free.

use super::*;

fn slot(index: usize) -> usize {
    CELL_VA_START + index * CELL_VA_STRIDE
}

/// An entropy source that has none (RV64, AArch64, x86 without RDRAND).
fn no_entropy(_: &mut [u8]) -> usize {
    0
}

/// A reproducible stand-in for RDSEED: splitmix64 over a shared counter.
fn seeded(buf: &mut [u8]) -> usize {
    static STATE: AtomicU64 = AtomicU64::new(0x5EED);
    let mut z = STATE.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    let len = buf.len().min(8);
    buf[..len].copy_from_slice(&z.to_le_bytes()[..len]);
    len
}

fn lowest_first() -> SlotOrder {
    let order = SlotOrder::new(no_entropy);
    assert!(
        !order.seed(),
        "no entropy must leave the order deterministic"
    );
    order
}

fn random_order() -> SlotOrder {
    let order = SlotOrder::new(seeded);
    assert!(order.seed(), "a working source must randomise the order");
    order
}

#[test]
fn reserve_refuses_a_slot_another_spawn_owns() {
    let order = lowest_first();
    let slots = SlotAllocator::new();
    let pie = slots.alloc(&order).expect("first slot");
    assert_eq!(pie, slot(0));
    assert!(
        !slots.reserve(pie),
        "fixed-VA image claimed a live PIE slot"
    );

    // Once released, the same slot is claimable exactly once.
    slots.free(pie);
    assert!(slots.reserve(pie));
    assert!(!slots.reserve(pie));
}

#[test]
fn reserve_ahead_of_the_lowest_free_slot_keeps_the_others_allocatable() {
    let order = lowest_first();
    let slots = SlotAllocator::new();
    assert!(slots.reserve(slot(3)));
    let mut handed = [
        slots.alloc(&order).unwrap(),
        slots.alloc(&order).unwrap(),
        slots.alloc(&order).unwrap(),
        slots.alloc(&order).unwrap(),
    ];
    handed.sort_unstable();
    assert_eq!(handed, [slot(0), slot(1), slot(2), slot(4)]);
    assert!(!slots.reserve(slot(1)), "a free slot was handed out twice");

    // Freeing the reservation returns it to circulation.
    slots.free(slot(3));
    assert_eq!(slots.alloc(&order), Some(slot(3)));
}

#[test]
fn reserve_refuses_non_slot_addresses() {
    let order = lowest_first();
    let slots = SlotAllocator::new();
    assert!(!slots.reserve(0));
    assert!(!slots.reserve(CELL_VA_START - CELL_VA_STRIDE));
    assert!(!slots.reserve(slot(0) + 0x1000));
    assert!(!slots.reserve(slot(MAX_SLOTS)));
    assert_eq!(slots.owned(), 0, "a refused claim must own nothing");
    assert_eq!(slots.alloc(&order), Some(slot(0)));
}

#[test]
fn a_zero_seed_source_keeps_the_lowest_first_order() {
    let order = lowest_first();
    let slots = SlotAllocator::new();
    for index in 0..6 {
        assert_eq!(slots.alloc(&order), Some(slot(index)));
    }
    // A freed slot below the highest one handed out is the next one taken,
    // then allocation continues above the highest (the old bump order).
    slots.free(slot(2));
    slots.free(slot(4));
    assert_eq!(slots.alloc(&order), Some(slot(2)));
    assert_eq!(slots.alloc(&order), Some(slot(4)));
    assert_eq!(slots.alloc(&order), Some(slot(6)));
    // Freeing the highest slot rewinds: it is the next one taken.
    slots.free(slot(6));
    assert_eq!(slots.alloc(&order), Some(slot(6)));
}

#[test]
fn a_seeded_allocator_never_hands_out_an_owned_slot() {
    let order = random_order();
    let slots = SlotAllocator::new();
    let mut held = [false; MAX_SLOTS];
    let mut first = [0; 16];
    let take = |held: &mut [bool; MAX_SLOTS], base: Option<usize>| {
        let base = base.expect("a slot is free");
        let index = CELLS.index_of_base(base).expect("a slot base");
        assert!(!held[index], "slot {index} handed out while owned");
        held[index] = true;
        index
    };
    for round in 0..MAX_SLOTS {
        let index = take(&mut held, slots.alloc(&order));
        if round < first.len() {
            first[round] = index;
        }
    }
    assert_ne!(
        first,
        core::array::from_fn::<usize, 16, _>(|index| index),
        "a seeded order must not be lowest-first"
    );
    assert_eq!(slots.alloc(&order), None, "every slot is held");
    assert_eq!(slots.owned(), MAX_SLOTS);

    // Churn: free a spread of slots, claim some of them back by address, and
    // let random starts take the rest; ownership tracks the model throughout.
    for round in 0..64 {
        for index in (round % 7..MAX_SLOTS).step_by(7) {
            slots.free(slot(index));
            held[index] = false;
        }
        for index in (round % 7..MAX_SLOTS).step_by(21) {
            assert!(slots.reserve(slot(index)), "free slot {index} refused");
            assert!(!slots.reserve(slot(index)), "slot {index} reserved twice");
            held[index] = true;
        }
        assert_eq!(slots.owned(), held.iter().filter(|h| **h).count());
        while held.iter().any(|h| !*h) {
            take(&mut held, slots.alloc(&order));
        }
        assert_eq!(slots.alloc(&order), None);
    }
}

#[test]
fn a_probe_from_any_start_takes_the_next_free_slot_wrapping() {
    let slots = SlotAllocator::new();
    let free = [5, 63, 64, 130, MAX_SLOTS - 1];
    for index in 0..MAX_SLOTS {
        if !free.contains(&index) {
            assert!(slots.reserve(slot(index)));
        }
    }
    for start in 0..MAX_SLOTS {
        let expected = free
            .iter()
            .copied()
            .find(|index| *index >= start)
            .unwrap_or(free[0]);
        assert_eq!(
            slots.claim_from(start),
            Some(slot(expected)),
            "start {start}"
        );
        slots.free(slot(expected));
    }
    assert_eq!(slots.owned(), MAX_SLOTS - free.len());
}

#[test]
fn a_partial_last_word_never_yields_a_slot_past_the_region() {
    const SLOTS: usize = 70;
    let region = Region {
        start: CELL_VA_START,
        stride: CELL_VA_STRIDE,
        slots: SLOTS,
    };
    let order = random_order();
    let slots: SlotAllocator<2> = SlotAllocator::over(region);
    let mut held = [false; SLOTS];
    for _ in 0..SLOTS {
        let base = slots.alloc(&order).expect("a slot is free");
        let index = region.index_of_base(base).expect("inside the region");
        assert!(!core::mem::replace(&mut held[index], true));
    }
    assert_eq!(slots.alloc(&order), None);
    assert_eq!(slots.claim_from(SLOTS - 1), None);
}

#[test]
fn slot_containing_maps_into_the_region_only() {
    assert_eq!(slot_containing(0), None);
    assert_eq!(slot_containing(CELL_VA_START - 1), None);
    assert_eq!(slot_containing(slot(2) + 0x1234), Some(slot(2)));
    assert_eq!(slot_containing(slot(MAX_SLOTS)), None);
}

#[test]
fn the_stack_arena_lies_above_the_cell_slots_and_recycles_its_slots() {
    let stack_pages = crate::task::STACK_PAGES + crate::task::stack::STACK_GUARD_PAGES;
    assert_eq!(STACK_VA_START, CELL_VA_START + MAX_SLOTS * CELL_VA_STRIDE);
    assert!(STACK_VA_STRIDE.is_power_of_two());
    assert!(stack_pages * crate::memory::paging::PAGE_SIZE <= STACK_VA_STRIDE);

    let order = lowest_first();
    let arena: SlotAllocator<{ STACK_SLOTS / 64 }> = SlotAllocator::over(STACKS);
    let first = arena.alloc(&order).expect("first stack slot");
    let second = arena.alloc(&order).expect("second stack slot");
    assert_eq!(
        (first, second),
        (STACK_VA_START, STACK_VA_START + STACK_VA_STRIDE)
    );
    assert!(
        slot_containing(first).is_none(),
        "a stack slot is no cell slot"
    );
    assert_eq!(arena.owned(), 2);

    // A returned slot is the next one handed out, and nothing leaks.
    arena.free(first);
    assert_eq!(arena.owned(), 1);
    assert_eq!(arena.alloc(&order), Some(first));
    arena.free(second);
    arena.free(first);
    assert_eq!(arena.owned(), 0);
    // An address inside a slot, or past the arena, is not a slot base.
    arena.free(first + crate::memory::paging::PAGE_SIZE);
    arena.free(STACK_VA_START + STACK_SLOTS * STACK_VA_STRIDE);
    assert_eq!(arena.owned(), 0);

    // Random starts stay inside the arena and do not take it lowest-first.
    let order = random_order();
    let taken: [usize; 8] = core::array::from_fn(|_| arena.alloc(&order).unwrap());
    assert!(taken.iter().all(|base| in_arena(*base)));
    assert_ne!(
        taken,
        core::array::from_fn::<usize, 8, _>(|index| STACK_VA_START + index * STACK_VA_STRIDE),
    );
    assert_eq!(arena.owned(), taken.len());
}

fn in_arena(base: usize) -> bool {
    STACKS.index_of_base(base).is_some()
}

#[test]
fn an_identity_range_overlaps_the_allocated_va_exactly_at_its_edges() {
    const PAGE: usize = crate::memory::paging::PAGE_SIZE;
    let end = STACK_VA_START + STACK_SLOTS * STACK_VA_STRIDE;
    // Below the cell slots, and from the arena's end up: free to identity-map.
    assert!(!overlaps_allocated_va(0, CELL_VA_START));
    assert!(!overlaps_allocated_va(CELL_VA_START - PAGE, PAGE));
    assert!(!overlaps_allocated_va(end, PAGE));
    assert!(!overlaps_allocated_va(end, usize::MAX - end));
    // Touching either region by one page, or straddling both.
    assert!(overlaps_allocated_va(CELL_VA_START - PAGE, 2 * PAGE));
    assert!(overlaps_allocated_va(STACK_VA_START, PAGE));
    assert!(overlaps_allocated_va(STACK_VA_START - PAGE, 2 * PAGE));
    assert!(overlaps_allocated_va(end - PAGE, PAGE));
    assert!(overlaps_allocated_va(0, end + PAGE));
    // A 64-bit BAR in the default q35 64-bit hole [4 GiB, 36 GiB) is clear;
    // one at 80 GiB is not.
    assert!(!overlaps_allocated_va(4 << 30, 32 << 30));
    assert!(overlaps_allocated_va(80 << 30, 1 << 20));
    // Empty ranges never overlap; an overflowing end always does.
    assert!(!overlaps_allocated_va(STACK_VA_START, 0));
    assert!(overlaps_allocated_va(usize::MAX - PAGE, 2 * PAGE));
}
