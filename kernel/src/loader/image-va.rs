//! Where a cell image's PT_LOAD pages may land.
//!
//! "Currently unmapped" is not ownership: the null page, every stack guard
//! page (kernel and user) and every other cell's not-yet-mapped slot pages
//! are unmapped yet owned by someone else. So every image — PIE or fixed-VA —
//! is confined to the one `va_alloc` slot its own spawn holds, checked before
//! a single frame is allocated:
//!
//! * the null page is refused outright (a kernel NULL dereference must fault);
//! * every page lies in `[CELL_VA_START, USER_VADDR_MAX)`;
//! * every page lies inside the spawn's slot, so it cannot reach a foreign
//!   slot even if that slot is still sparsely mapped;
//! * no page overlaps a live stack run (`task::stack` reservations), which
//!   matters wherever identity-mapped RAM reaches the cell region.
//!
//! A fixed-VA image gets its slot from the lowest PT_LOAD page
//! ([`fixed_image_slot`]); `va_alloc::reserve_cell_va` then refuses it when
//! another spawn owns that slot.

use crate::loader::va_alloc;
use crate::memory::paging::PAGE_SIZE;
use types::{ViError, ViResult};

/// The user VA window's upper bound, from the HAL (`hal::paging::USER_LIMIT`,
/// 256 GiB on every 64-bit target). Cells compiled at 4 GiB+ slots are well
/// inside it. On 32-bit targets the address space itself bounds the checked
/// arithmetic, because `usize::MAX` cannot represent 4 GiB.
#[cfg(cellos_wide)]
pub(crate) const USER_VADDR_MAX: usize = crate::hal::paging::USER_LIMIT;
#[cfg(not(cellos_wide))]
pub(crate) const USER_VADDR_MAX: usize = usize::MAX;

/// The slot `[base, end)` a spawn owns.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ImageSlot {
    base: usize,
    end: usize,
}

impl ImageSlot {
    /// `None` unless `base` is a slot base `va_alloc` can hand out.
    pub(crate) fn of(base: usize) -> Option<Self> {
        Some(Self {
            base,
            end: va_alloc::slot_end(base)?,
        })
    }
}

/// The slot a fixed-VA image is linked into, from its lowest PT_LOAD address.
/// Images below the cell region (including VA 0) have no slot.
pub(crate) fn fixed_image_slot(lowest_vaddr: usize) -> ViResult<usize> {
    va_alloc::slot_containing(lowest_vaddr & !(PAGE_SIZE - 1)).ok_or_else(|| {
        log::error!(
            "ELF: fixed-VA image at {lowest_vaddr:#x} is outside the cell VA region; \
             link it inside one cell slot or build it PIE"
        );
        ViError::PermissionDenied
    })
}

/// Admit one PT_LOAD occupying bytes `[start, end)` and return its page count.
///
/// `reserved(page_start, page_end)` reports an overlap with a kernel-owned
/// unmapped range (stack runs in production).
pub(crate) fn admit_segment(
    start: usize,
    end: usize,
    slot: ImageSlot,
    reserved: impl Fn(usize, usize) -> bool,
) -> ViResult<usize> {
    if end < start {
        return Err(ViError::InvalidInput);
    }
    let page_start = start & !(PAGE_SIZE - 1);
    let page_end = end
        .checked_add(PAGE_SIZE - 1)
        .ok_or(ViError::InvalidInput)?
        & !(PAGE_SIZE - 1);
    let refuse = |why: &str| {
        log::error!("ELF: PT_LOAD {start:#x}..{end:#x} refused: {why}");
        Err(ViError::PermissionDenied)
    };
    if page_start == 0 {
        return refuse("maps the null page");
    }
    if page_start < va_alloc::CELL_VA_START || page_end > USER_VADDR_MAX {
        return refuse("outside the cell VA region");
    }
    // `start >= slot.end` catches an empty segment sitting on the slot's end.
    if start >= slot.end || page_start < slot.base || page_end > slot.end {
        return refuse("leaves the VA slot this spawn owns");
    }
    if reserved(page_start, page_end) {
        return refuse("overlaps a live stack or guard page");
    }
    Ok((page_end - page_start) / PAGE_SIZE)
}

#[cfg(test)]
#[path = "image-va-tests.rs"]
mod tests;
