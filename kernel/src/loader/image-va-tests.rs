//! Host tests: no image page reaches the null page, a stack guard page or a
//! slot its spawn does not own.

use super::*;
use crate::loader::va_alloc::{CELL_VA_START, CELL_VA_STRIDE};
use crate::task::stack::reservations::StackReservations;

const NOTHING_RESERVED: fn(usize, usize) -> bool = |_, _| false;

fn own_slot() -> ImageSlot {
    ImageSlot::of(CELL_VA_START + CELL_VA_STRIDE).expect("slot 1")
}

#[test]
fn null_page_is_refused() {
    for (start, end) in [(0, 0x10), (0x10, 0x2000), (0, 0)] {
        assert_eq!(
            admit_segment(start, end, own_slot(), NOTHING_RESERVED),
            Err(ViError::PermissionDenied),
            "PT_LOAD {start:#x}..{end:#x}"
        );
    }
    assert_eq!(fixed_image_slot(0), Err(ViError::PermissionDenied));
    assert_eq!(fixed_image_slot(0x40_0000), Err(ViError::PermissionDenied));
}

#[test]
fn foreign_slot_overlap_is_refused() {
    let own = CELL_VA_START + CELL_VA_STRIDE;
    let next = own + CELL_VA_STRIDE;
    // Spilling one byte into the next slot, landing wholly in it, or starting
    // in the previous one.
    for (start, end) in [
        (next - 0x1000, next + 1),
        (next, next + 0x1000),
        (own - 0x1000, own + 0x1000),
        (next, next),
    ] {
        assert_eq!(
            admit_segment(start, end, own_slot(), NOTHING_RESERVED),
            Err(ViError::PermissionDenied),
            "PT_LOAD {start:#x}..{end:#x}"
        );
    }
    // Exactly filling the own slot is admitted.
    assert_eq!(
        admit_segment(own, next, own_slot(), NOTHING_RESERVED),
        Ok(CELL_VA_STRIDE / PAGE_SIZE)
    );
    assert_eq!(fixed_image_slot(own + 0x1234), Ok(own));
}

#[test]
fn stack_guard_overlap_is_refused() {
    let own = CELL_VA_START + CELL_VA_STRIDE;
    // A stack whose identity-mapped run falls inside the slot (RAM above
    // 4 GiB): its bottom guard page is unmapped but owned. Production passes
    // `task::stack::overlaps_stack_reservation`, a view of the same registry.
    let runs = StackReservations::new();
    let stack_base = own + 0x10_0000;
    let stack_top = stack_base + 4 * PAGE_SIZE;
    runs.insert(stack_base, stack_top).unwrap();
    let reserved = |start, end| runs.overlaps(start, end);
    let guard = (stack_base, stack_base + PAGE_SIZE);
    assert_eq!(
        admit_segment(guard.0, guard.1, own_slot(), reserved),
        Err(ViError::PermissionDenied)
    );
    assert_eq!(
        admit_segment(stack_base - 0x10, stack_base + 1, own_slot(), reserved),
        Err(ViError::PermissionDenied)
    );
    // The page right below the run is still the image's to use.
    assert_eq!(
        admit_segment(stack_base - PAGE_SIZE, stack_base, own_slot(), reserved),
        Ok(1)
    );
    assert!(runs.remove(stack_base, stack_top));
    assert_eq!(admit_segment(guard.0, guard.1, own_slot(), reserved), Ok(1));
}

#[test]
fn outside_user_half_is_refused() {
    let wide = ImageSlot {
        base: CELL_VA_START,
        end: usize::MAX,
    };
    assert_eq!(
        admit_segment(
            USER_VADDR_MAX - 0x1000,
            USER_VADDR_MAX + 1,
            wide,
            NOTHING_RESERVED
        ),
        Err(ViError::PermissionDenied)
    );
    assert_eq!(
        admit_segment(
            CELL_VA_START - 0x1000,
            CELL_VA_START,
            wide,
            NOTHING_RESERVED
        ),
        Err(ViError::PermissionDenied)
    );
}
