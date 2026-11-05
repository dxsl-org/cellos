//! Host tests for slot ownership: a fixed-VA image can claim only a slot no
//! other spawn holds, and claimed slots never come back out of `alloc`.

use super::*;

fn slot(index: usize) -> usize {
    CELL_VA_START + index * CELL_VA_STRIDE
}

#[test]
fn reserve_refuses_a_slot_another_spawn_owns() {
    let slots = SlotAllocator::new();
    let pie = slots.alloc().expect("first slot");
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
fn reserve_ahead_of_bump_keeps_skipped_slots_allocatable() {
    let slots = SlotAllocator::new();
    assert!(slots.reserve(slot(3)));
    let mut handed = [
        slots.alloc().unwrap(),
        slots.alloc().unwrap(),
        slots.alloc().unwrap(),
        slots.alloc().unwrap(),
    ];
    handed.sort_unstable();
    assert_eq!(handed, [slot(0), slot(1), slot(2), slot(4)]);
    assert!(!slots.reserve(slot(1)), "skipped slot was handed out twice");

    // Freeing the reservation returns it to circulation.
    slots.free(slot(3));
    assert_eq!(slots.alloc(), Some(slot(3)));
}

#[test]
fn reserve_refuses_non_slot_addresses() {
    let slots = SlotAllocator::new();
    assert!(!slots.reserve(0));
    assert!(!slots.reserve(CELL_VA_START - CELL_VA_STRIDE));
    assert!(!slots.reserve(slot(0) + 0x1000));
    assert!(!slots.reserve(slot(MAX_SLOTS)));
    assert_eq!(
        slots.alloc(),
        Some(slot(0)),
        "a refused claim must not move bump"
    );
}

#[test]
fn slot_containing_maps_into_the_region_only() {
    assert_eq!(slot_containing(0), None);
    assert_eq!(slot_containing(CELL_VA_START - 1), None);
    assert_eq!(slot_containing(slot(2) + 0x1234), Some(slot(2)));
    assert_eq!(slot_containing(slot(MAX_SLOTS)), None);
}
