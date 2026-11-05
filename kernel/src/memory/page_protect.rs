//! Change the permission bits of an ALREADY-MAPPED page.
//!
//! Distinct from `paging::map_page`, which installs (or replaces) a translation:
//! these entry points keep the existing VA→PA binding and rewrite only the
//! access-rights bits, then invalidate the stale TLB entries.
//!
//! - `protect_page`: one page, one invalidation of that VA.
//! - `protect_range`: `protect_page` per page of a contiguous range.
//! - `protect_pages` (the W^X lowering pass): on x86_64 every leaf of the list
//!   is written first under `KERNEL_ROOT`, then one acknowledged invalidation
//!   covers each contiguous run of written pages before it returns, on the error
//!   path too; the other arches keep `protect_page` per page.
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
//!   - x86_64: all-PCID/global local invalidation followed by every online CPU's
//!     acknowledged local invalidation; missing acknowledgement is fail-closed.
//!   - AArch64: `flush_tlb_page` broadcasts a stage-1 TLBI
//!     (`tlbi vaae1is`, plus `vae2is` when EL2 is active) inside the required
//!     barrier pair, but this repo still treats two-PE runtime proof as gated
//!     evidence rather than marking D7 complete.
//! - Bare-physical arches return `NotSupported` instead of pretending W^X
//!   exists.
//!
//! Re-exported as `memory::paging::{protect_page, protect_pages, protect_range}`
//! — it lives in its own file only to keep `paging.rs` from growing further.

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
    let page_va = vaddr & !(PAGE_SIZE - 1);
    let root_lock = KERNEL_ROOT.lock();
    let root_phys = (*root_lock).ok_or(PageTableError::NotSupported)?;
    rewrite_leaf_x86(root_phys, page_va, new_flags)?;
    crate::memory::tlb_shootdown::flush_page(page_va);
    Ok(())
}

/// The leaf rewrite of [`protect_page`] without its invalidation. The caller
/// holds `KERNEL_ROOT` (whose value is `root_phys`) and owns the flush.
#[cfg(target_arch = "x86_64")]
fn rewrite_leaf_x86(
    root_phys: types::PhysAddr,
    page_va: VAddr,
    new_flags: Flags,
) -> PagingResult<()> {
    use crate::memory::frame::phys_to_virt;
    use hal::paging::{walk_create, walk_read, PTE_ADDR_MASK, PTE_PCD, PTE_PRESENT, PTE_PWT};

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
    Ok(())
}

/// Rewrite the permission bits of every page in `pages`, in order, as one batch
/// (the W^X lowering pass, `loader::wx::enforce`).
///
/// Concurrency contract (phase 07b R1, owner grant
/// `s2-phase07b-flush-batching-grant.md`):
/// - x86_64: `KERNEL_ROOT` is held from the first leaf write through the last
///   flush, as [`protect_page`] holds it through its own. Every leaf is written
///   first, then ONE `tlb_shootdown::flush_range` covers each contiguous run of
///   written pages. `flush_range` returns only after every online CPU
///   acknowledged; an unconfirmed CPU is its fail-stop panic, unchanged. So a
///   lowering (W→R, X removal) is in force on every online CPU before this
///   returns — before the Cell can be registered to run, and before a failed
///   spawn's teardown can free a frame.
/// - The error path flushes the pages written before the failure, too: no
///   caller ever sees a rewritten leaf whose invalidation is still pending.
/// - riscv64/aarch64 (and the bare-physical arches): [`protect_page`] per page
///   in order, exactly the pass this replaces.
///
/// # Errors
/// The first page that could not be rewritten, with the [`protect_page`] error.
/// The pages before it keep their new flags (see [`protect_range`]).
#[cfg(target_arch = "x86_64")]
pub fn protect_pages(pages: &[(VAddr, Flags)]) -> Result<(), (VAddr, PageTableError)> {
    let Some(&(first, _)) = pages.first() else {
        return Ok(());
    };
    let root_lock = KERNEL_ROOT.lock();
    let root_phys = (*root_lock).ok_or((first, PageTableError::NotSupported))?;
    let result = write_then_flush(
        pages,
        |page_va, flags| rewrite_leaf_x86(root_phys, page_va, flags),
        crate::memory::tlb_shootdown::flush_range,
    );
    drop(root_lock);
    result
}

/// See the x86_64 [`protect_pages`]: these arches keep the per-page pass.
#[cfg(not(target_arch = "x86_64"))]
pub fn protect_pages(pages: &[(VAddr, Flags)]) -> Result<(), (VAddr, PageTableError)> {
    for &(va, flags) in pages {
        protect_page(va, flags).map_err(|error| (va, error))?;
    }
    Ok(())
}

/// Write each page's leaf in order (stopping at the first failure), then call
/// `flush` once per contiguous run of the pages actually written, before
/// returning either way. The error carries the caller's address for the page.
#[cfg(target_arch = "x86_64")]
fn write_then_flush<E>(
    pages: &[(VAddr, Flags)],
    mut write: impl FnMut(VAddr, Flags) -> Result<(), E>,
    mut flush: impl FnMut(VAddr, usize),
) -> Result<(), (VAddr, E)> {
    let mut written = 0;
    let mut failure = None;
    for &(va, flags) in pages {
        if let Err(error) = write(va & !(PAGE_SIZE - 1), flags) {
            failure = Some((va, error));
            break;
        }
        written += 1;
    }
    let written_pages = pages[..written]
        .iter()
        .map(|&(va, _)| va & !(PAGE_SIZE - 1));
    for (start, size) in contiguous_runs(written_pages) {
        flush(start, size);
    }
    failure.map_or(Ok(()), Err)
}

/// Coalesce page addresses, in the order given, into maximal runs of
/// consecutive pages: `(start, size_in_bytes)`. A page that does not directly
/// follow the previous one starts a new run, so every page is in exactly one.
#[cfg(target_arch = "x86_64")]
fn contiguous_runs(pages: impl Iterator<Item = VAddr>) -> impl Iterator<Item = (VAddr, usize)> {
    let mut pages = pages.peekable();
    core::iter::from_fn(move || {
        let start = pages.next()?;
        let mut size = PAGE_SIZE;
        while let Some(next) = start.checked_add(size) {
            if pages.next_if_eq(&next).is_none() {
                break;
            }
            size += PAGE_SIZE;
        }
        Some((start, size))
    })
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

#[cfg(all(test, target_arch = "x86_64"))]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    #[derive(Debug, PartialEq, Eq)]
    enum Event {
        Write(VAddr),
        Flush(VAddr, usize),
    }

    fn ro() -> Flags {
        Flags::from_bits(Flags::VALID | Flags::USER | Flags::READ)
    }

    fn page(n: usize) -> VAddr {
        0x1_0000_0000 + n * PAGE_SIZE
    }

    /// Drive `write_then_flush` with a recorder; `fail_at` makes that page's
    /// write fail.
    fn run(
        pages: &[(VAddr, Flags)],
        fail_at: Option<VAddr>,
    ) -> (Result<(), (VAddr, ())>, Vec<Event>) {
        let events = core::cell::RefCell::new(Vec::new());
        let result = write_then_flush(
            pages,
            |va, _| {
                if Some(va) == fail_at {
                    return Err(());
                }
                events.borrow_mut().push(Event::Write(va));
                Ok(())
            },
            |start, size| events.borrow_mut().push(Event::Flush(start, size)),
        );
        (result, events.into_inner())
    }

    /// Each written page is covered by exactly one flush issued after it.
    fn covered_once_after_write(events: &[Event], written: &[VAddr]) {
        for &va in written {
            let write_at = events.iter().position(|e| *e == Event::Write(va)).unwrap();
            let covering: Vec<usize> = events
                .iter()
                .enumerate()
                .filter(|(_, e)| matches!(e, Event::Flush(s, n) if (*s..*s + *n).contains(&va)))
                .map(|(i, _)| i)
                .collect();
            assert_eq!(covering.len(), 1, "page {va:#x} flushed once");
            assert!(
                covering[0] > write_at,
                "page {va:#x} flushed after its write"
            );
        }
    }

    #[test]
    fn contiguous_runs_coalesce_only_adjacent_pages() {
        let runs = |pages: &[VAddr]| contiguous_runs(pages.iter().copied()).collect::<Vec<_>>();
        assert_eq!(runs(&[]), []);
        assert_eq!(runs(&[page(0)]), [(page(0), PAGE_SIZE)]);
        // An ELF image: R-X 0..3, R-- 3..5 (adjacent), RW- after a one-page gap.
        let image: Vec<VAddr> = (0..5).chain(6..9).map(page).collect();
        assert_eq!(
            runs(&image),
            [(page(0), 5 * PAGE_SIZE), (page(6), 3 * PAGE_SIZE)]
        );
        // Order is kept: a descending pair is two runs, never a merged one.
        assert_eq!(
            runs(&[page(1), page(0)]),
            [(page(1), PAGE_SIZE), (page(0), PAGE_SIZE)]
        );
        // A run may end exactly at the last page boundary below 2^64. A run
        // that contained the last page would end at 2^64, which `flush_range`
        // refuses as a wrapping range; Cell pages lie far below it (the
        // coalescer's `checked_add` alone would not wrap there either).
        let top = usize::MAX & !(PAGE_SIZE - 1);
        assert_eq!(
            runs(&[top - 2 * PAGE_SIZE, top - PAGE_SIZE]),
            [(top - 2 * PAGE_SIZE, 2 * PAGE_SIZE)]
        );
    }

    #[test]
    fn lowering_writes_every_leaf_then_flushes_each_run_before_return() {
        // The vfs shape: thousands of pages in a few contiguous segments.
        let pages: Vec<(VAddr, Flags)> = (0..4000)
            .chain(4001..4486)
            .map(|n| (page(n), ro()))
            .collect();
        let (result, events) = run(&pages, None);
        assert_eq!(result, Ok(()));
        let flushes: Vec<&Event> = events
            .iter()
            .filter(|e| matches!(e, Event::Flush(..)))
            .collect();
        assert_eq!(
            flushes,
            [
                &Event::Flush(page(0), 4000 * PAGE_SIZE),
                &Event::Flush(page(4001), 485 * PAGE_SIZE)
            ]
        );
        // Every leaf is written before the first flush.
        let first_flush = events
            .iter()
            .position(|e| matches!(e, Event::Flush(..)))
            .unwrap();
        assert_eq!(first_flush, pages.len());
        covered_once_after_write(&events, &pages.iter().map(|p| p.0).collect::<Vec<_>>());
    }

    #[test]
    fn failed_lowering_flushes_the_written_prefix_before_returning() {
        let pages: Vec<(VAddr, Flags)> = (0..8).map(|n| (page(n), ro())).collect();
        let (result, events) = run(&pages, Some(page(5)));
        assert_eq!(result, Err((page(5), ())));
        assert_eq!(events.last(), Some(&Event::Flush(page(0), 5 * PAGE_SIZE)));
        covered_once_after_write(&events, &(0..5).map(page).collect::<Vec<_>>());
        // Nothing after the failed page was written or flushed.
        assert!(!events
            .iter()
            .any(|e| matches!(e, Event::Write(va) if *va >= page(5))));

        // A failure on the first page wrote nothing, so nothing is flushed.
        let (result, events) = run(&pages, Some(page(0)));
        assert_eq!(result, Err((page(0), ())));
        assert!(events.is_empty());
    }

    #[test]
    fn unaligned_addresses_are_written_and_flushed_as_their_page() {
        let pages = [(page(2) + 0x10, ro()), (page(3) + 0xfff, ro())];
        let (result, events) = run(&pages, None);
        assert_eq!(result, Ok(()));
        assert_eq!(
            events,
            [
                Event::Write(page(2)),
                Event::Write(page(3)),
                Event::Flush(page(2), 2 * PAGE_SIZE)
            ]
        );
        // The error names the caller's address, not the rounded page.
        let (result, _) = run(&pages, Some(page(3)));
        assert_eq!(result, Err((page(3) + 0xfff, ())));
    }
}
