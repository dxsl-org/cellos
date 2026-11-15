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
//!   calling hart the moment the call returns. A remote hart that does not
//!   confirm within the probe bound fails the call closed
//!   (`InvalidationUnconfirmed`): the PTE is rewritten, but the caller must
//!   not report the change as in force.
//! - Cross-hart / cross-core scope is architecture-specific today:
//!   - RV64: a changed PTE is ordered locally, invalidated with `sfence.vma`,
//!     then invalidated synchronously on online remote harts through SBI RFENCE.
//!     Firmware without RFENCE keeps the kernel single-hart; two-hart runtime
//!     evidence remains a separate gate.
//!   - x86_64: all-PCID/global local invalidation followed by every online CPU's
//!     acknowledged local invalidation; a missing acknowledgement is the
//!     `InvalidationUnconfirmed` error.
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
/// because a missing `VALID` bit unmaps the page rather than restricting it
/// (riscv64, aarch64; x86_64 always keeps the page present, and keeps the
/// frame's cache and global bits).
///
/// # Errors
/// - `PageTableError::InvalidAddress` — `vaddr` is not currently mapped.
/// - `PageTableError::NotSupported` — paging is inactive, or the target is a
///   bare-physical arch (riscv32 / x86_32 / arm32) with no page tables.
#[cfg(cellos_wide)]
pub fn protect_page(vaddr: VAddr, new_flags: Flags) -> PagingResult<()> {
    let page_va = vaddr & !(PAGE_SIZE - 1);
    // Before any rewrite: a hart that holds the frame allocator may not wait
    // for the flush, so it must not change the leaf either.
    let mut cap =
        crate::memory::frame::FlushCap::take().ok_or(PageTableError::InvalidationUnconfirmed)?;
    let root_lock = KERNEL_ROOT.lock();
    let root_phys = (*root_lock).ok_or(PageTableError::NotSupported)?;
    rewrite_leaf(root_phys, page_va, new_flags)?;
    let outcome = crate::memory::tlb_shootdown::flush_page(&mut cap, page_va);
    drop(root_lock);
    confirmed(outcome, page_va)
}

/// `outcome` as the protect result, logging an unconfirmed one once the
/// caller's `KERNEL_ROOT` is released (it ranks above the console).
#[cfg(cellos_wide)]
fn confirmed(outcome: crate::memory::frame_run::FlushOutcome, page_va: VAddr) -> PagingResult<()> {
    match outcome {
        crate::memory::frame_run::FlushOutcome::Confirmed => Ok(()),
        crate::memory::frame_run::FlushOutcome::Unconfirmed { cause, .. } => {
            log::error!(
                "[tlb] shared invalidation unconfirmed: permission change at {:#x} not in force everywhere ({:?})",
                page_va,
                cause
            );
            Err(PageTableError::InvalidationUnconfirmed)
        }
    }
}

/// The leaf rewrite of [`protect_page`] without its invalidation. The caller
/// holds `KERNEL_ROOT` (whose value is `root_phys`) and owns the flush.
///
/// Reading the frame back from the table is what makes this a permission
/// change rather than a remap: the caller has no say in the target frame.
#[cfg(cellos_wide)]
fn rewrite_leaf(root_phys: types::PhysAddr, page_va: VAddr, new_flags: Flags) -> PagingResult<()> {
    use hal::PageTableTrait;

    // SAFETY: root_phys is the live kernel root and the caller holds its lock.
    let table = unsafe { &mut *crate::memory::paging::table_at(root_phys) };
    table
        .protect(page_va, new_flags)
        .map_err(|error| match error {
            types::ViError::NotFound => PageTableError::InvalidAddress,
            // Every table on the path exists; an allocation request is a surprise
            // walk, reported as it always was.
            _ => PageTableError::OutOfMemory,
        })
}

/// Rewrite the permission bits of every page in `pages`, in order, as one batch
/// (the W^X lowering pass, `loader::wx::enforce`).
///
/// Concurrency contract (phase 07b R1, owner grant
/// `s2-phase07b-flush-batching-grant.md`):
/// - x86_64: `KERNEL_ROOT` is held from the first leaf write through the last
///   flush, as [`protect_page`] holds it through its own. Every leaf is written
///   first, then ONE `tlb_shootdown::flush_range` covers each contiguous run of
///   written pages. `flush_range` returns `Confirmed` only after every online
///   CPU acknowledged; an unconfirmed run is the `InvalidationUnconfirmed`
///   error (every run is still flushed). So on `Ok` a lowering (W→R, X
///   removal) is in force on every online CPU — before the Cell can be
///   registered to run, and before a failed spawn's teardown can free a frame.
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
    let mut cap = crate::memory::frame::FlushCap::take()
        .ok_or((first, PageTableError::InvalidationUnconfirmed))?;
    let root_lock = KERNEL_ROOT.lock();
    let root_phys = (*root_lock).ok_or((first, PageTableError::NotSupported))?;
    let mut unconfirmed = None;
    let result = write_then_flush(
        pages,
        |page_va, flags| rewrite_leaf(root_phys, page_va, flags),
        |start, size| match crate::memory::tlb_shootdown::flush_range(&mut cap, start, size) {
            crate::memory::frame_run::FlushOutcome::Confirmed => Ok(()),
            outcome => {
                unconfirmed.get_or_insert(outcome);
                Err(PageTableError::InvalidationUnconfirmed)
            }
        },
    );
    drop(root_lock);
    if let (Some(outcome), Err((va, _))) = (unconfirmed, result) {
        // Logs it; the error is already the result.
        let _ = confirmed(outcome, va);
    }
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

/// Rewrite the permission bits of every *mapped* page of the `pages` pages
/// from page-aligned `start`, then invalidate the whole range on every hart
/// with ONE shared flush, before returning either way (one step of the MMIO
/// lowering, `paging::restore_mmio_run`).
///
/// - An absent leaf is skipped, not an error, but the flush still covers it:
///   a translation removed without one may still be cached.
/// - A leaf that cannot be rewritten does not stop the pass: every other page
///   is still lowered and flushed, so a failure never leaves more pages
///   reachable than it must.
/// - `KERNEL_ROOT` is held from the first leaf write through the flush, as
///   [`protect_pages`] holds it.
/// - `await_flush`: wait for every hart to confirm the flush. An unconfirmed
///   flush clears it (and is logged), so the later steps of the same lowering
///   only send their requests: the lowering has already failed, and a silent
///   hart would otherwise cost each step a full probe bound.
///
/// Returns the pages rewritten.
///
/// # Errors
/// The first page that could not be rewritten, else `(start,
/// InvalidationUnconfirmed)` when the flush is not confirmed on every hart
/// (without `await_flush`, whenever a remote hart was asked).
#[cfg(cellos_wide)]
pub fn protect_mapped_range(
    start: VAddr,
    pages: usize,
    new_flags: Flags,
    await_flush: &mut bool,
) -> Result<usize, (VAddr, PageTableError)> {
    debug_assert_eq!(start & (PAGE_SIZE - 1), 0);
    if pages == 0 {
        return Ok(0);
    }
    let mut cap = if *await_flush {
        Some(
            crate::memory::frame::FlushCap::take()
                .ok_or((start, PageTableError::InvalidationUnconfirmed))?,
        )
    } else {
        None
    };
    let root_lock = KERNEL_ROOT.lock();
    let root_phys = (*root_lock).ok_or((start, PageTableError::NotSupported))?;
    let mut unconfirmed = None;
    let result = write_mapped_then_flush(
        start,
        pages,
        |page_va| match rewrite_leaf(root_phys, page_va, new_flags) {
            Ok(()) => Ok(true),
            Err(PageTableError::InvalidAddress) => Ok(false),
            Err(error) => Err(error),
        },
        |from, size| {
            let outcome = match cap.as_mut() {
                Some(cap) => crate::memory::tlb_shootdown::flush_range(cap, from, size),
                None => {
                    crate::memory::tlb_shootdown::flush_ranges_without_wait(&[(from, size)], false)
                }
            };
            match outcome {
                crate::memory::frame_run::FlushOutcome::Confirmed => Ok(()),
                outcome => {
                    unconfirmed = Some(outcome);
                    Err(PageTableError::InvalidationUnconfirmed)
                }
            }
        },
    );
    drop(root_lock);
    if let (Some(outcome), true) = (unconfirmed, cap.is_some()) {
        *await_flush = false;
        // Logs it; the result already carries the failure.
        let _ = confirmed(outcome, start);
    }
    result
}

/// Write the leaf of each of the `pages` pages from `start` (`write` returns
/// `false` for an absent leaf), every page even after one failed, then call
/// `flush` once over the whole range. Returns the pages written; the error is
/// the first failed write, else the flush's (named by `start`).
#[cfg(cellos_wide)]
fn write_mapped_then_flush<E>(
    start: VAddr,
    pages: usize,
    mut write: impl FnMut(VAddr) -> Result<bool, E>,
    flush: impl FnOnce(VAddr, usize) -> Result<(), E>,
) -> Result<usize, (VAddr, E)> {
    let mut written = 0;
    let mut failure = None;
    for page_va in (0..pages).map(|page| start + page * PAGE_SIZE) {
        match write(page_va) {
            Ok(true) => written += 1,
            Ok(false) => {}
            Err(error) => {
                failure.get_or_insert((page_va, error));
            }
        }
    }
    let flushed = flush(start, pages * PAGE_SIZE).map_err(|error| (start, error));
    match failure {
        Some(failure) => Err(failure),
        None => flushed.map(|()| written),
    }
}

/// Write each page's leaf in order (stopping at the first failure), then call
/// `flush` once per contiguous run of the pages actually written, before
/// returning either way — every run is flushed even after one failed. The
/// error carries the caller's address for the page: a write failure first,
/// else the start of the first run whose flush failed.
#[cfg(target_arch = "x86_64")]
fn write_then_flush<E>(
    pages: &[(VAddr, Flags)],
    mut write: impl FnMut(VAddr, Flags) -> Result<(), E>,
    mut flush: impl FnMut(VAddr, usize) -> Result<(), E>,
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
    let mut flush_failure = None;
    for (start, size) in contiguous_runs(written_pages) {
        if let Err(error) = flush(start, size) {
            flush_failure.get_or_insert((start, error));
        }
    }
    failure.or(flush_failure).map_or(Ok(()), Err)
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
            |start, size| {
                events.borrow_mut().push(Event::Flush(start, size));
                Ok(())
            },
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
    fn an_unconfirmed_run_fails_the_pass_but_every_run_is_still_flushed() {
        // Two runs; the first one's flush is not confirmed.
        let pages: Vec<(VAddr, Flags)> = [0, 1, 5].into_iter().map(|n| (page(n), ro())).collect();
        let flushed = core::cell::RefCell::new(Vec::new());
        let result = write_then_flush(
            &pages,
            |_, _| Ok::<(), &str>(()),
            |start, size| {
                flushed.borrow_mut().push((start, size));
                if start == page(0) {
                    Err("unconfirmed")
                } else {
                    Ok(())
                }
            },
        );
        assert_eq!(result, Err((page(0), "unconfirmed")));
        assert_eq!(
            flushed.into_inner(),
            [(page(0), 2 * PAGE_SIZE), (page(5), PAGE_SIZE)]
        );
    }

    #[test]
    fn a_mapped_range_skips_absent_leaves_and_flushes_the_whole_range_once() {
        let absent = |va: VAddr| va == page(2) || va == page(3);
        let flushes = core::cell::RefCell::new(Vec::new());
        let flush = |start, size| {
            flushes.borrow_mut().push((start, size));
            Ok::<(), &str>(())
        };
        let result = write_mapped_then_flush(page(0), 6, |va| Ok(!absent(va)), flush);
        assert_eq!(result, Ok(4));
        // The absent leaves are flushed with the rest: one flush, whole range.
        assert_eq!(flushes.take(), [(page(0), 6 * PAGE_SIZE)]);

        // Nothing mapped at all still flushes the range.
        let result = write_mapped_then_flush(page(2), 2, |va| Ok(!absent(va)), flush);
        assert_eq!(result, Ok(0));
        assert_eq!(flushes.take(), [(page(2), 2 * PAGE_SIZE)]);
    }

    #[test]
    fn a_failed_leaf_does_not_stop_the_mapped_range_and_an_unconfirmed_flush_fails_it() {
        let written = core::cell::RefCell::new(Vec::new());
        let flushes = core::cell::RefCell::new(Vec::new());
        let result = write_mapped_then_flush(
            page(0),
            4,
            |va| {
                if va == page(1) {
                    return Err("oom");
                }
                written.borrow_mut().push(va);
                Ok(true)
            },
            |start, size| {
                flushes.borrow_mut().push((start, size));
                Ok(())
            },
        );
        // Every other page is still lowered and flushed; the error names the page.
        assert_eq!(result, Err((page(1), "oom")));
        assert_eq!(written.take(), [page(0), page(2), page(3)]);
        assert_eq!(flushes.take(), [(page(0), 4 * PAGE_SIZE)]);

        let result = write_mapped_then_flush(page(4), 2, |_| Ok(true), |_, _| Err("unconfirmed"));
        assert_eq!(result, Err((page(4), "unconfirmed")));
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
