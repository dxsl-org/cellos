//! Virtual address allocators: cell image slots and the user-stack arena.
//!
//! Assigns each cell image one aligned VA slot within the cell VA region
//! [CELL_VA_START, CELL_VA_START + MAX_SLOTS * CELL_VA_STRIDE): a PIE takes
//! any free slot (`alloc_cell_va`); a fixed-VA image claims the one slot it is
//! linked into (`reserve_cell_va`). Every PT_LOAD must stay inside the slot
//! its spawn owns (see `loader::elf::image_va`).
//!
//! Every user stack takes one `STACK_VA_STRIDE` slot of the stack arena right
//! above the cell region (`alloc_stack_va`), so it is never mapped at its
//! physical address; the slot returns (`free_stack_va`) only once its frames'
//! translations are gone on every hart.
//!
//! Both regions live entirely below the SV39 user-half boundary (256 GiB), so
//! the `USER_VADDR_MAX` guard is never triggered by any allocated slot.
//!
//! # Design
//! Simple two-level allocator: a bump pointer for never-allocated slots, and a
//! free list (atomic bitset) for slots returned by `free_cell_va`.  Allocations
//! are O(n_freed) in the worst case, O(1) when the free list is empty.

// RV32 lacks native 64-bit atomics; portable-atomic polyfills AtomicU64 there
// via the critical-section impl hal/arch/riscv registers.
#[cfg(not(target_arch = "riscv32"))]
use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;
#[cfg(target_arch = "riscv32")]
use portable_atomic::AtomicU64;

#[cfg(all(feature = "large-cell-va", target_pointer_width = "32"))]
compile_error!("large-cell-va requires a 64-bit paged target");
#[cfg(all(feature = "large-cell-va", feature = "cell-scale-experiment"))]
compile_error!(
    "large-cell-va cannot be combined with cell-scale-experiment: exceeds SV39 user half"
);

/// Cell VA region start — 64 GiB (0x10_0000_0000).
///
/// Must lie above ALL identity-mapped regions in the SAS, avoiding collision
/// when physical RAM extends to >= 4 GiB (e.g. 0x1_0000_0000..0x1_8000_0000):
///   - QEMU virt RISC-V: UART/VirtIO MMIO at 0x1000_0000–0x1001_0000
///   - PCIe ECAM at 0x3000_0000 (1 MiB); VirtIO BAR below 0x8000_0000
///   - RAM identity map: supports physical configurations up to 48 GiB
///   - QEMU virt AArch64: GIC/peripheral MMIO below 0x1000_0000
///     64 GiB clears all of the above with room to spare and remains well inside
///     the user window (`hal::paging::USER_LIMIT`, 256 GiB).
///
/// RISC-V medany: even the 256 MiB desktop slot stays within ±2 GiB.
///
/// RV32 Nano boots with SATP=0 (bare physical, no paging — see specs/04), so
/// PIE cells never occur there and `alloc_cell_va` is never actually called;
/// this constant only needs to type-check as a 32-bit `usize` on that target.
#[cfg(not(target_arch = "riscv32"))]
pub(crate) const CELL_VA_START: usize = 0x10_0000_0000;
#[cfg(target_arch = "riscv32")]
pub(crate) const CELL_VA_START: usize = 0x2000_0000;

/// Embedded keeps the historical 32 MiB spacing. Desktop images opt in to a
/// 256 MiB slot, sufficient for a 128 MiB static arena plus code and data.
#[cfg(not(feature = "large-cell-va"))]
pub(crate) const CELL_VA_STRIDE: usize = 32 * 1024 * 1024;
#[cfg(feature = "large-cell-va")]
pub(crate) const CELL_VA_STRIDE: usize = 256 * 1024 * 1024;

/// Maximum simultaneous PIE cell slots: normally 512 (16 GiB embedded or
/// 128 GiB desktop). Both regions, starting at 64 GiB, fit SV39's user half.
///
/// `cell-scale-experiment` raises this to 4096 slots (4096 × 32 MiB = 128 GiB of
/// VA, still inside the user half) so the D5 scale measurement measures memory
/// rather than this constant. See `memory::cell_quota::MAX_CELLS`.
#[cfg(not(feature = "cell-scale-experiment"))]
const MAX_SLOTS: usize = 512;
#[cfg(feature = "cell-scale-experiment")]
const MAX_SLOTS: usize = 4096;

#[cfg(cellos_wide)]
const _: () = assert!(CELL_VA_START + MAX_SLOTS * CELL_VA_STRIDE <= crate::hal::paging::USER_LIMIT);

/// A run of equal, aligned VA slots: `slots` slots of `stride` bytes from
/// `start`.
#[derive(Clone, Copy)]
pub(super) struct Region {
    start: usize,
    stride: usize,
    slots: usize,
}

impl Region {
    /// Index of the slot whose base is exactly `base`.
    fn index_of_base(&self, base: usize) -> Option<usize> {
        let offset = base.checked_sub(self.start)?;
        (offset.is_multiple_of(self.stride) && offset / self.stride < self.slots)
            .then_some(offset / self.stride)
    }

    fn base(&self, index: usize) -> usize {
        self.start + index * self.stride
    }
}

/// The cell image slots.
const CELLS: Region = Region {
    start: CELL_VA_START,
    stride: CELL_VA_STRIDE,
    slots: MAX_SLOTS,
};

/// Exclusive end of a valid reservation, shared by ELF admission and release.
pub(crate) fn slot_end(base: usize) -> Option<usize> {
    CELLS.index_of_base(base)?;
    base.checked_add(CELL_VA_STRIDE)
}

/// Base of the slot containing `va`, or `None` outside the cell VA region.
pub(crate) fn slot_containing(va: usize) -> Option<usize> {
    let index = va.checked_sub(CELL_VA_START)? / CELL_VA_STRIDE;
    (index < MAX_SLOTS).then(|| CELL_VA_START + index * CELL_VA_STRIDE)
}

/// Number of AtomicU64 words needed to cover MAX_SLOTS bits.
const BITMAP_WORDS: usize = MAX_SLOTS.div_ceil(64);

/// Slot ownership over one [`Region`] (`WORDS` bitmap words cover its slots).
/// Invariant: a slot is owned by exactly one holder iff its index is below
/// `bump` and its `free` bit is clear; every `free` bit lies below `bump`.
pub(super) struct SlotAllocator<const WORDS: usize = BITMAP_WORDS> {
    region: Region,
    /// The first slot that has NEVER been handed out.
    bump: core::sync::atomic::AtomicUsize,
    /// Bit N = 1 means slot N is available for reuse. Each repeat of the inline
    /// `const` block is a distinct AtomicU64 (an inline const sidesteps
    /// `clippy::declare_interior_mutable_const` without changing semantics).
    free: [AtomicU64; WORDS],
}

static SLOTS: SlotAllocator = SlotAllocator::new();

impl SlotAllocator {
    /// The cell image slots.
    pub(super) const fn new() -> Self {
        Self::over(CELLS)
    }
}

impl<const WORDS: usize> SlotAllocator<WORDS> {
    pub(super) const fn over(region: Region) -> Self {
        assert!(region.slots <= WORDS * 64);
        Self {
            region,
            bump: core::sync::atomic::AtomicUsize::new(0),
            free: [const { AtomicU64::new(0) }; WORDS],
        }
    }

    pub(super) fn alloc(&self) -> Option<usize> {
        // 1. Try the free list first.
        for (word_idx, word) in self.free.iter().enumerate() {
            let mut val = word.load(Ordering::Relaxed);
            while val != 0 {
                let bit = val.trailing_zeros() as usize;
                let slot = word_idx * 64 + bit;
                if slot >= self.region.slots {
                    break;
                }
                let mask = 1u64 << bit;
                match word.compare_exchange_weak(
                    val,
                    val & !mask,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => return Some(self.region.base(slot)),
                    Err(cur) => val = cur, // retry with updated value
                }
            }
        }

        // 2. Bump-allocate a fresh slot.
        let idx = self.bump.fetch_add(1, Ordering::SeqCst);
        if idx >= self.region.slots {
            self.bump.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        Some(self.region.base(idx))
    }

    /// Claim the exact slot at `base`, or refuse when another holder owns it.
    pub(super) fn reserve(&self, base: usize) -> bool {
        let Some(slot) = self.region.index_of_base(base) else {
            return false;
        };
        loop {
            let bump = self.bump.load(Ordering::SeqCst);
            if slot < bump {
                // Handed out before: claimable only while it is on the free list.
                let mask = 1u64 << (slot % 64);
                return self.free[slot / 64].fetch_and(!mask, Ordering::AcqRel) & mask != 0;
            }
            if self
                .bump
                .compare_exchange(bump, slot + 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                // The slots jumped over were never handed out: they go on the
                // free list so the ownership invariant keeps holding.
                for skipped in bump..slot {
                    self.free[skipped / 64].fetch_or(1u64 << (skipped % 64), Ordering::Release);
                }
                return true;
            }
        }
    }

    pub(super) fn free(&self, base: usize) {
        let Some(slot) = self.region.index_of_base(base) else {
            return;
        };
        // Restore the bump cursor exactly when this is the most recently minted
        // slot. If another allocation raced ahead, publish it to the free bitmap.
        if self
            .bump
            .compare_exchange(slot + 1, slot, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            let word = &self.free[slot / 64];
            let mask = 1u64 << (slot % 64);
            word.fetch_or(mask, Ordering::Release);
        }
    }

    /// Slots owned right now: below `bump` and not on the free list.
    #[cfg(test)]
    pub(super) fn owned(&self) -> usize {
        let freed: u32 = self
            .free
            .iter()
            .map(|word| word.load(Ordering::Acquire).count_ones())
            .sum();
        self.bump.load(Ordering::Acquire) - freed as usize
    }

    #[cfg(feature = "test-hooks")]
    fn snapshot(&self) -> (usize, alloc::vec::Vec<u64>) {
        (
            self.bump.load(Ordering::Acquire),
            self.free
                .iter()
                .map(|word| word.load(Ordering::Acquire))
                .collect(),
        )
    }
}

/// First byte of the user-stack arena: right above the cell slots, so it lies
/// inside the user window and above every identity-mapped byte (checked at
/// boot by [`assert_stack_arena_above`]). 80 GiB by default, 192 GiB with
/// `large-cell-va` or `cell-scale-experiment`.
#[cfg(cellos_wide)]
pub(crate) const STACK_VA_START: usize = CELL_VA_START + MAX_SLOTS * CELL_VA_STRIDE;

/// One user stack per slot: its bottom guards, then its usable pages, then
/// unmapped VA up to the next slot. The largest stack the kernel allocates
/// (`task::STACK_PAGES` plus `task::stack::STACK_GUARD_PAGES`) rounded up to a
/// power of two: 512 KiB.
pub(crate) const STACK_VA_STRIDE: usize =
    (crate::task::STACK_PAGES + crate::task::stack::STACK_GUARD_PAGES).next_power_of_two()
        * crate::memory::paging::PAGE_SIZE;

/// Live user stacks at once (16 GiB of VA at the 512 KiB stride).
#[cfg(cellos_wide)]
const STACK_SLOTS: usize = 32 * 1024;

#[cfg(cellos_wide)]
const _: () =
    assert!(STACK_VA_START + STACK_SLOTS * STACK_VA_STRIDE <= crate::hal::paging::USER_LIMIT);

#[cfg(cellos_wide)]
const STACKS: Region = Region {
    start: STACK_VA_START,
    stride: STACK_VA_STRIDE,
    slots: STACK_SLOTS,
};

#[cfg(cellos_wide)]
static STACK_ARENA: SlotAllocator<{ STACK_SLOTS / 64 }> = SlotAllocator::over(STACKS);

/// Take one stack slot (`STACK_VA_STRIDE` bytes of VA, nothing mapped), or
/// `None` when every slot is held.
#[cfg(cellos_wide)]
pub(crate) fn alloc_stack_va() -> Option<usize> {
    STACK_ARENA.alloc()
}

/// Paging targets only: the bare-physical ones never map a stack.
#[cfg(not(cellos_wide))]
pub(crate) fn alloc_stack_va() -> Option<usize> {
    None
}

/// Return a stack slot. Only once nothing translates any page of it on any
/// hart: the stack's `VaHold` calls this after its run's flush is confirmed.
pub(crate) fn free_stack_va(base: usize) {
    #[cfg(cellos_wide)]
    STACK_ARENA.free(base);
    #[cfg(not(cellos_wide))]
    let _ = base;
}

/// Boot check: everything mapped at VA == PA (RAM, whose grants and SHM pages
/// keep their physical address as VA, and the boot MMIO windows) ends at or
/// below `identity_end`, which must not reach the stack arena.
#[cfg(cellos_wide)]
pub(crate) fn assert_stack_arena_above(identity_end: usize) {
    assert!(
        identity_end <= STACK_VA_START,
        "identity-mapped range ends at {identity_end:#x}, inside the stack arena at {STACK_VA_START:#x}"
    );
}

/// End of the VA the kernel hands out from its allocators: the cell slots
/// `[CELL_VA_START, STACK_VA_START)`, then the stack arena right above them.
#[cfg(cellos_wide)]
const ALLOCATED_VA_END: usize = STACK_VA_START + STACK_SLOTS * STACK_VA_STRIDE;

/// Does `[start, start + len)` reach the VA the kernel hands out from its
/// allocators, `[CELL_VA_START, ALLOCATED_VA_END)` (cell slots and stack
/// arena)? A physical range that does can never be mapped at VA == PA: its
/// leaves would alias a cell image or a user stack. A range whose end
/// overflows counts as overlapping; an empty one never does.
#[cfg(cellos_wide)]
pub(crate) fn overlaps_allocated_va(start: usize, len: usize) -> bool {
    match start.checked_add(len) {
        Some(end) => len != 0 && start < ALLOCATED_VA_END && CELL_VA_START < end,
        None => true,
    }
}

/// Paging targets only: the bare-physical ones allocate no VA.
#[cfg(not(cellos_wide))]
pub(crate) fn overlaps_allocated_va(_start: usize, _len: usize) -> bool {
    false
}

/// Does `va` lie in the stack arena?
#[cfg(all(cellos_wide, feature = "test-hooks"))]
pub(crate) fn in_stack_arena(va: usize) -> bool {
    (STACK_VA_START..STACK_VA_START + STACK_SLOTS * STACK_VA_STRIDE).contains(&va)
}

/// The stack arena's allocator state (`bump`, free bitmap), as [`snapshot`]
/// is for the cell slots.
#[cfg(all(cellos_wide, feature = "test-hooks"))]
pub(crate) fn stack_snapshot() -> (usize, alloc::vec::Vec<u64>) {
    STACK_ARENA.snapshot()
}

/// Allocate one profile-sized VA slot for a new PIE cell.
///
/// Returns `None` when all slots are exhausted (unlikely in practice: 512 cells
/// and the free list recycles dead cells' slots).
pub fn alloc_cell_va() -> Option<usize> {
    SLOTS.alloc()
}

/// Claim the slot at `base` for a fixed-VA (ET_EXEC) image that is linked
/// inside it. `false` when `base` is not a slot base or another cell owns it:
/// a fixed-VA image may never land in a slot a PIE (or another image) holds.
pub fn reserve_cell_va(base: usize) -> bool {
    SLOTS.reserve(base)
}

/// Return a VA slot to the free list so it can be reused by a future cell.
///
/// `base` must be a value previously returned by `alloc_cell_va` or claimed
/// by `reserve_cell_va`. Silently ignores invalid values.
pub fn free_cell_va(base: usize) {
    SLOTS.free(base)
}

#[cfg(feature = "test-hooks")]
pub(crate) fn snapshot() -> (usize, alloc::vec::Vec<u64>) {
    SLOTS.snapshot()
}

#[cfg(test)]
#[path = "va-alloc-tests.rs"]
mod tests;
