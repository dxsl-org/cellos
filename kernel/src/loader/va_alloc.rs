//! Virtual address allocator for PIE cells.
//!
//! Assigns each PIE cell a 32 MiB-aligned VA slot within the cell VA region
//! [CELL_VA_START, CELL_VA_START + MAX_SLOTS * CELL_VA_STRIDE).
//!
//! This region lives entirely below the SV39 user-half boundary (256 GiB), so
//! elf.rs's `USER_VADDR_MAX` guard is never triggered by any allocated slot.
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

/// Cell VA region start — 4 GiB (0x1_0000_0000).
///
/// Must lie above ALL identity-mapped regions in the SAS:
///   - QEMU virt RISC-V: UART/VirtIO MMIO at 0x1000_0000–0x1001_0000
///   - PCIe ECAM at 0x3000_0000 (1 MiB); VirtIO BAR below 0x8000_0000
///   - RAM identity map: 256 MB at 0x8000_0000 → ends at 0x8FFF_FFFF
///   - QEMU virt AArch64: GIC/peripheral MMIO below 0x1000_0000
///     4 GiB clears all of the above with room to spare and remains well inside
///     the SV39 user-half (256 GiB = 0x40_0000_0000).
///
/// RISC-V medany: intra-cell refs are ≤32 MiB apart → always within ±2 GiB.
///
/// RV32 Nano boots with SATP=0 (bare physical, no paging — see specs/04), so
/// PIE cells never occur there and `alloc_cell_va` is never actually called;
/// this constant only needs to type-check as a 32-bit `usize` on that target.
#[cfg(not(target_arch = "riscv32"))]
const CELL_VA_START: usize = 0x1_0000_0000;
#[cfg(target_arch = "riscv32")]
const CELL_VA_START: usize = 0x2000_0000;

/// Each cell slot is 32 MiB — same spacing as the old static VA assignments,
/// large enough for code + data + stack for any current cell.
const CELL_VA_STRIDE: usize = 0x200_0000;

/// Maximum simultaneous PIE cell slots — 512 × 32 MiB = 16 GiB of cell VA.
/// Well within SV39 user half (256 GiB) and leaves room for future extensions.
const MAX_SLOTS: usize = 512;

/// Number of AtomicU64 words needed to cover MAX_SLOTS bits.
const BITMAP_WORDS: usize = MAX_SLOTS.div_ceil(64);

/// Bump index: the first slot that has NEVER been allocated.
static BUMP: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

/// Free-list bitmap: bit N = 1 means slot N is available for reuse.
/// Uses relaxed CAS loops — no ordering guarantee needed beyond the CAS itself.
// Each repeat evaluates the inline `const` block independently, giving
// BITMAP_WORDS distinct AtomicU64 instances (not one instance aliased across
// slots) — an inline const sidesteps `clippy::declare_interior_mutable_const`,
// which would otherwise flag a *named* const of this type, without changing
// the initialization semantics.
static FREE: [AtomicU64; BITMAP_WORDS] = [const { AtomicU64::new(0) }; BITMAP_WORDS];

/// Allocate a 32 MiB VA base for a new PIE cell.
///
/// Returns `None` when all slots are exhausted (unlikely in practice: 512 cells
/// and the free list recycles dead cells' slots).
pub fn alloc_cell_va() -> Option<usize> {
    // 1. Try the free list first.
    for (word_idx, word) in FREE.iter().enumerate() {
        let mut val = word.load(Ordering::Relaxed);
        while val != 0 {
            let bit = val.trailing_zeros() as usize;
            let slot = word_idx * 64 + bit;
            if slot >= MAX_SLOTS {
                break;
            }
            let mask = 1u64 << bit;
            match word.compare_exchange_weak(val, val & !mask, Ordering::AcqRel, Ordering::Relaxed)
            {
                Ok(_) => return Some(CELL_VA_START + slot * CELL_VA_STRIDE),
                Err(cur) => val = cur, // retry with updated value
            }
        }
    }

    // 2. Bump-allocate a fresh slot.
    let idx = BUMP.fetch_add(1, Ordering::SeqCst);
    if idx >= MAX_SLOTS {
        BUMP.fetch_sub(1, Ordering::SeqCst);
        return None;
    }
    Some(CELL_VA_START + idx * CELL_VA_STRIDE)
}

/// Return a VA slot to the free list so it can be reused by a future cell.
///
/// `base` must be a value previously returned by `alloc_cell_va`.
/// Silently ignores invalid values (out of range or misaligned).
pub fn free_cell_va(base: usize) {
    if base < CELL_VA_START {
        return;
    }
    let offset = base - CELL_VA_START;
    if !offset.is_multiple_of(CELL_VA_STRIDE) {
        return;
    }
    let slot = offset / CELL_VA_STRIDE;
    if slot >= MAX_SLOTS {
        return;
    }
    let word = &FREE[slot / 64];
    let mask = 1u64 << (slot % 64);
    word.fetch_or(mask, Ordering::Release);
}
