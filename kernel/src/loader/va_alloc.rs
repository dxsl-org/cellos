//! Virtual address allocator for cell images.
//!
//! Assigns each cell image one aligned VA slot within the cell VA region
//! [CELL_VA_START, CELL_VA_START + MAX_SLOTS * CELL_VA_STRIDE): a PIE takes
//! any free slot (`alloc_cell_va`); a fixed-VA image claims the one slot it is
//! linked into (`reserve_cell_va`). Every PT_LOAD must stay inside the slot
//! its spawn owns (see `loader::elf::image_va`).
//!
//! This region lives entirely below the SV39 user-half boundary (256 GiB), so
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

/// Exclusive end of a valid reservation, shared by ELF admission and release.
pub(crate) fn slot_end(base: usize) -> Option<usize> {
    let offset = base.checked_sub(CELL_VA_START)?;
    if !offset.is_multiple_of(CELL_VA_STRIDE) || offset / CELL_VA_STRIDE >= MAX_SLOTS {
        return None;
    }
    base.checked_add(CELL_VA_STRIDE)
}

/// Base of the slot containing `va`, or `None` outside the cell VA region.
pub(crate) fn slot_containing(va: usize) -> Option<usize> {
    let index = va.checked_sub(CELL_VA_START)? / CELL_VA_STRIDE;
    (index < MAX_SLOTS).then(|| CELL_VA_START + index * CELL_VA_STRIDE)
}

/// Number of AtomicU64 words needed to cover MAX_SLOTS bits.
const BITMAP_WORDS: usize = MAX_SLOTS.div_ceil(64);

/// Slot ownership. Invariant: a slot is owned by exactly one spawn iff its
/// index is below `bump` and its `free` bit is clear; every `free` bit lies
/// below `bump`.
pub(super) struct SlotAllocator {
    /// The first slot that has NEVER been handed out.
    bump: core::sync::atomic::AtomicUsize,
    /// Bit N = 1 means slot N is available for reuse. Each repeat of the inline
    /// `const` block is a distinct AtomicU64 (an inline const sidesteps
    /// `clippy::declare_interior_mutable_const` without changing semantics).
    free: [AtomicU64; BITMAP_WORDS],
}

static SLOTS: SlotAllocator = SlotAllocator::new();

impl SlotAllocator {
    pub(super) const fn new() -> Self {
        Self {
            bump: core::sync::atomic::AtomicUsize::new(0),
            free: [const { AtomicU64::new(0) }; BITMAP_WORDS],
        }
    }

    pub(super) fn alloc(&self) -> Option<usize> {
        // 1. Try the free list first.
        for (word_idx, word) in self.free.iter().enumerate() {
            let mut val = word.load(Ordering::Relaxed);
            while val != 0 {
                let bit = val.trailing_zeros() as usize;
                let slot = word_idx * 64 + bit;
                if slot >= MAX_SLOTS {
                    break;
                }
                let mask = 1u64 << bit;
                match word.compare_exchange_weak(
                    val,
                    val & !mask,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => return Some(CELL_VA_START + slot * CELL_VA_STRIDE),
                    Err(cur) => val = cur, // retry with updated value
                }
            }
        }

        // 2. Bump-allocate a fresh slot.
        let idx = self.bump.fetch_add(1, Ordering::SeqCst);
        if idx >= MAX_SLOTS {
            self.bump.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        Some(CELL_VA_START + idx * CELL_VA_STRIDE)
    }

    /// Claim the exact slot at `base`, or refuse when another spawn owns it.
    pub(super) fn reserve(&self, base: usize) -> bool {
        if slot_end(base).is_none() {
            return false;
        }
        let slot = (base - CELL_VA_START) / CELL_VA_STRIDE;
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
        if slot_end(base).is_none() {
            return;
        }
        let slot = (base - CELL_VA_START) / CELL_VA_STRIDE;
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
    (
        SLOTS.bump.load(Ordering::Acquire),
        SLOTS
            .free
            .iter()
            .map(|word| word.load(Ordering::Acquire))
            .collect(),
    )
}

#[cfg(test)]
#[path = "va-alloc-tests.rs"]
mod tests;
