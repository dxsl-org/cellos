//! Change the permission bits of an ALREADY-MAPPED page.
//!
//! Distinct from `paging::map_page`, which installs (or replaces) a translation:
//! these entry points keep the existing VA→PA binding and rewrite only the
//! access-rights bits, then invalidate the stale TLB entry for that one VA.
//!
//! Contract shared by every arch leg:
//! - An unmapped VA is an error (`InvalidAddress`), never a silent map.
//! - The physical frame is read back from the live page table, so the caller
//!   cannot accidentally re-point the page while "only" changing flags.
//! - The per-VA TLB invalidate happens BEFORE returning. A caller that lowers
//!   permissions may therefore assume the new rights are in force on the
//!   calling hart the moment the call returns.
//! - Cross-hart / cross-core scope is architecture-specific today:
//!   - RV64: a changed PTE is ordered locally, invalidated with `sfence.vma`,
//!     then invalidated synchronously on online remote harts through SBI RFENCE.
//!     Firmware without RFENCE keeps the kernel single-hart; two-hart runtime
//!     evidence remains a separate gate.
//!   - x86_64: `invlpg` is local only; there is no SMP IPI shootdown path yet.
//!   - AArch64: `flush_tlb_page` broadcasts a stage-1 TLBI
//!     (`tlbi vaae1is`, plus `vae2is` when EL2 is active) inside the required
//!     barrier pair, but this repo still treats two-PE runtime proof as gated
//!     evidence rather than marking D7 complete.
//! - Bare-physical arches return `NotSupported` instead of pretending W^X
//!   exists.
//!
//! Re-exported as `memory::paging::{protect_page, protect_range}` — it lives in
//! its own file only to keep `paging.rs` from growing further.

use crate::memory::paging::{Flags, PagingResult, PAGE_SIZE};
#[allow(unused_imports)] // used by the paged arches only
use crate::memory::paging::{PageTableError, KERNEL_ROOT};
use types::VAddr;

/// Rewrite the permission bits of one mapped 4 KiB page and invalidate its TLB entry.
///
/// `new_flags` REPLACES the old flags wholesale — it is not OR-ed in. Pass the
/// complete desired set (including `VALID` / `USER` / `ACCESSED` / `DIRTY`),
/// because a missing `VALID` bit unmaps the page rather than restricting it.
///
/// # Errors
/// - `PageTableError::InvalidAddress` — `vaddr` is not currently mapped.
/// - `PageTableError::NotSupported` — paging is inactive, or the target is a
///   bare-physical arch (riscv32 / x86_32 / arm32) with no page tables.
#[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
pub fn protect_page(vaddr: VAddr, new_flags: Flags) -> PagingResult<()> {
    use hal::{PageTable, PageTableTrait};

    let page_va = vaddr & !(PAGE_SIZE - 1);
    let root_lock = KERNEL_ROOT.lock();
    let root_phys = (*root_lock).ok_or(PageTableError::NotSupported)?;
    // SAFETY: KERNEL_ROOT holds the physical address of the live root table,
    // which is identity-mapped on both of these arches, and the lock we hold
    // gives us exclusive access to it for the duration of this function.
    let root_table = unsafe { &mut *(root_phys as *mut PageTable) };

    // Reading the frame back from the table is what makes this a permission
    // change rather than a remap: the caller has no say in the target frame.
    let phys = root_table
        .translate(page_va)
        .ok_or(PageTableError::InvalidAddress)?
        & !(PAGE_SIZE - 1);

    // Every intermediate table on this path already exists (the page is mapped),
    // so `map` never invokes the allocator; a None-returning closure documents
    // that and turns any surprise walk into OutOfMemory instead of a silent alloc.
    let mut no_alloc = || None;
    root_table
        .map(page_va, phys, new_flags, &mut no_alloc)
        .map_err(|_| PageTableError::OutOfMemory)?;

    crate::memory::tlb_shootdown::flush_page(page_va);
    Ok(())
}

/// x86_64 leg: rewrite the leaf PTE's flag bits, preserving the frame address
/// and the cache-attribute bits (PWT/PCD) already recorded for the page.
#[cfg(target_arch = "x86_64")]
pub fn protect_page(vaddr: VAddr, new_flags: Flags) -> PagingResult<()> {
    use crate::memory::frame::phys_to_virt;
    use hal::paging::{walk_create, walk_read, PTE_ADDR_MASK, PTE_PCD, PTE_PRESENT, PTE_PWT};

    let page_va = vaddr & !(PAGE_SIZE - 1);
    let root_lock = KERNEL_ROOT.lock();
    let root_phys = (*root_lock).ok_or(PageTableError::NotSupported)?;

    // SAFETY: root_phys is the kernel's live PML4; phys_to_virt maps it into the
    // HHDM window, which is readable/writable from Ring 0 for the whole boot.
    let existing = unsafe { walk_read(phys_to_virt(root_phys) as *const u64, page_va) }
        .ok_or(PageTableError::InvalidAddress)?;
    if existing & PTE_PRESENT == 0 {
        return Err(PageTableError::InvalidAddress);
    }

    let cache_bits = existing & (PTE_PWT | PTE_PCD);
    let new_pte = (existing & PTE_ADDR_MASK) | cache_bits | pte_bits_from_flags(new_flags);

    let mut no_alloc = || None::<usize>;
    // SAFETY: the same PML4, re-walked for a mutable leaf pointer. The page is
    // present, so no intermediate table is missing and `no_alloc` is never called.
    let pte_ptr =
        unsafe { walk_create(phys_to_virt(root_phys) as *mut u64, page_va, &mut no_alloc) }
            .ok_or(PageTableError::InvalidAddress)?;
    // SAFETY: pte_ptr is the leaf PTE slot for page_va, obtained from the walker.
    unsafe {
        core::ptr::write_volatile(pte_ptr, new_pte);
    }

    crate::memory::tlb_shootdown::flush_page(page_va);
    Ok(())
}

/// Translate generic `PageFlags` into x86_64 leaf-PTE bits.
///
/// `EXECUTE` is inverted into the NX bit: absence of EXECUTE means NX is SET.
/// Shared with `paging::map_page` so a page created and a page re-protected
/// cannot disagree about what a given `Flags` value means.
#[cfg(target_arch = "x86_64")]
pub(crate) fn pte_bits_from_flags(flags: Flags) -> u64 {
    use hal::paging::{PTE_NX, PTE_PRESENT, PTE_USER, PTE_WRITABLE};
    let bits = flags.bits();
    let mut pte = PTE_PRESENT;
    if bits & hal::PageFlags::WRITE != 0 {
        pte |= PTE_WRITABLE;
    }
    if bits & hal::PageFlags::USER != 0 {
        pte |= PTE_USER;
    }
    if bits & hal::PageFlags::EXECUTE == 0 {
        pte |= PTE_NX;
    }
    pte
}

/// Bare-physical arches have no PTEs to protect; the caller must treat W^X as
/// unavailable rather than silently believing it was applied.
#[cfg(any(target_arch = "riscv32", target_arch = "x86", target_arch = "arm"))]
pub fn protect_page(_vaddr: VAddr, _new_flags: Flags) -> PagingResult<()> {
    Err(PageTableError::NotSupported)
}

/// Apply [`protect_page`] to `pages` consecutive 4 KiB pages starting at `start`.
///
/// Fails fast: the first page that is not mapped aborts the walk and the pages
/// already lowered KEEP their new flags. Callers must therefore treat an error
/// as "permissions are in an unknown but never MORE permissive state" — this is
/// the safe direction for a W^X lowering pass and the reason no rollback exists.
///
/// # Errors
/// Propagates the first [`protect_page`] failure.
pub fn protect_range(start: VAddr, pages: usize, new_flags: Flags) -> PagingResult<()> {
    for i in 0..pages {
        protect_page(start + i * PAGE_SIZE, new_flags)?;
    }
    Ok(())
}
