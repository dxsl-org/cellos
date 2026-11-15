//! A cell image's segment frames and its VA slot (architecture remediation
//! Phase 16: moved from `task/stack.rs`, next to the release contract its
//! teardown follows).
//!
//! The image's frames, the kernel-root tables its mappings needed and its VA
//! slot are released as one operation: every leaf and every detached table is
//! invalidated by one shared flush, and the slot stays reserved until the
//! frames behind it are freed — or, when a hart does not confirm, quarantined
//! with them (census risk 2: a slot handed out again while a remote hart may
//! still translate it would resolve the new image's VA to the old frame).

use crate::memory::cell_quota::Charge;
use crate::memory::frame_run::{Batch, FrameRun, KernelAlias, RunKind, VaHold};
use crate::memory::paging::{self, PAGE_SIZE};
use types::VAddr;

/// Physical frames mapped for a cell's ELF segments, recorded as `(vaddr, frame)`
/// at load time so they can be reclaimed when the cell dies.
///
/// `Stack::drop` only frees stacks; without this a cell's code/data frames leak
/// on every death (a supervised service restarted repeatedly would grow to OOM).
/// Segment frames are allocated exclusively for the cell by `load_segments`
/// (IPC/shared buffers use separate frames), so freeing them on death is safe.
#[derive(Debug)]
pub struct CellSegments {
    pages: alloc::vec::Vec<(types::VAddr, types::PhysAddr)>,
    /// ELF pages whose final mapping grants the Cell write permission.
    writable_pages: alloc::vec::Vec<types::VAddr>,
    /// VA slot this image owns (`va_alloc::alloc_cell_va` for PIE,
    /// `va_alloc::reserve_cell_va` for fixed-VA images); `0` for kernel-built
    /// test segments. Returned to the allocator's free list on drop.
    va_slot: usize,
    /// The image's BTI decision (`loader::gnu_property::arm`): its executable
    /// pages are guarded (stage-1 GP). Carried here so a Tier-2 private root
    /// built from these segments (`memory::address_space::create_cell_domain`)
    /// guards them too, not only the shared-address-space mapping.
    guarded_code: bool,
    #[cfg(feature = "p01-trace")]
    pub(crate) p01_obligation: Option<crate::p01_resources::Obligation>,
}

impl CellSegments {
    pub fn new(pages: alloc::vec::Vec<(types::VAddr, types::PhysAddr)>, va_slot: usize) -> Self {
        Self {
            pages,
            writable_pages: alloc::vec::Vec::new(),
            va_slot,
            guarded_code: false,
            #[cfg(feature = "p01-trace")]
            p01_obligation: None,
        }
    }

    /// Callers size both vectors exactly (`try_reserve_exact`), so construction
    /// never reallocates: a shrink here would be an infallible allocation on the
    /// path documented as infallible, and would move buffers p01 already adopted.
    pub fn with_writable_pages(
        pages: alloc::vec::Vec<(types::VAddr, types::PhysAddr)>,
        mut writable_pages: alloc::vec::Vec<types::VAddr>,
        va_slot: usize,
    ) -> Self {
        // Sorted for the binary searches below; in place, so no allocation.
        writable_pages.sort_unstable();
        writable_pages.dedup();
        Self {
            pages,
            writable_pages,
            va_slot,
            guarded_code: false,
            #[cfg(feature = "p01-trace")]
            p01_obligation: None,
        }
    }

    /// Record the image's BTI decision (see the `guarded_code` field).
    pub fn with_guarded_code(mut self, guarded_code: bool) -> Self {
        self.guarded_code = guarded_code;
        self
    }

    /// Whether the image's executable pages are BTI-guarded.
    pub fn guarded_code(&self) -> bool {
        self.guarded_code
    }

    pub fn pages(&self) -> &[(types::VAddr, types::PhysAddr)] {
        &self.pages
    }

    /// Exact capacities of this task's exclusive segment metadata, not frames.
    /// Publication calls this while holding SCHEDULER with the final identity.
    #[cfg(feature = "p01-trace")]
    pub(crate) fn p01_backing(&self, owner: crate::p01_measurement::Owner) {
        crate::memory::heap::p01_owner::runtime_vec(
            &self.pages,
            owner,
            crate::memory::heap::p01_owner::TASK_SEGMENTS,
        );
        crate::memory::heap::p01_owner::runtime_vec(
            &self.writable_pages,
            owner,
            crate::memory::heap::p01_owner::TASK_SEGMENTS,
        );
    }

    pub fn is_writable(&self, va: types::VAddr) -> bool {
        self.writable_pages.binary_search(&va).is_ok()
    }
    /// Return the exclusive end of the writable page containing `ptr`.
    pub fn writable_page_end_containing(&self, ptr: usize) -> Option<usize> {
        let page = ptr & !(PAGE_SIZE - 1);
        self.writable_pages
            .binary_search(&page)
            .ok()
            .and_then(|_| page.checked_add(PAGE_SIZE))
    }

    /// Whether every page touched by this range is writable in this Cell image.
    pub fn contains_writable_range(&self, ptr: usize, len: usize) -> bool {
        let Some(end) = ptr.checked_add(len) else {
            return false;
        };
        if len == 0 {
            return false;
        }
        let mut page = ptr & !(PAGE_SIZE - 1);
        while page < end {
            if self.writable_pages.binary_search(&page).is_err() {
                return false;
            }
            page = match page.checked_add(PAGE_SIZE) {
                Some(next) => next,
                None => return false,
            };
        }
        true
    }

    #[cfg(feature = "test-hooks")]
    pub(crate) fn unpublished_pages(&self) -> &[(types::VAddr, types::PhysAddr)] {
        &self.pages
    }

    /// Total physical bytes reserved for this cell's ELF segment frames.
    pub fn allocated_bytes(&self) -> usize {
        self.pages.len() * PAGE_SIZE
    }

    /// Unmap this cell's segment VAs immediately at death — WITHOUT freeing the
    /// frames (those are freed lazily when the zombie is reaped, in `drop`).
    ///
    /// Frees the address space right away so (a) a respawn can reuse the fixed VA
    /// and (b) the load-time overwrite guard (`load_segments`) only ever observes
    /// LIVE cells' (and kernel MMIO) mappings, never a dead-but-unreaped cell's.
    /// Locks only `KERNEL_ROOT` (a leaf), so it is safe under the SCHEDULER lock.
    /// One shared invalidation covers every page, and it is not waited for
    /// (census F4: the caller holds `SCHEDULER`): nothing is freed here, the
    /// frames and the VA slot stay owned, and the drop invalidates every VA
    /// again and frees them only once that flush confirms.
    pub fn eager_unmap(&self) {
        let mut batch = Batch::new();
        self.unmap_into(&mut batch);
        let _ = batch.complete_without_wait();
    }

    /// Unmap every VA that still resolves to this image's frame and record
    /// every VA in `batch`.
    fn unmap_into(&self, batch: &mut Batch) {
        for &(vaddr, frame) in &self.pages {
            // Only unmap a VA that still resolves to OUR frame (it won't if a
            // respawn already re-pointed it — leave the new mapping intact).
            if paging::virt_to_phys(vaddr) == Some(frame) {
                let _ = paging::unmap_page(batch, vaddr);
            }
            // Invalidate even when a replacement mapping won the race: an old
            // remote translation for this VA can still point at this frame.
            batch.invalidate(vaddr);
        }
    }

    /// The teardown up to its flush: unmap, detach every emptied table and
    /// retire the backing (the last run holding the VA slot) into `batch`.
    fn teardown_into(&self, batch: &mut Batch) {
        self.unmap_into(batch);
        for &(vaddr, _frame) in &self.pages {
            paging::prune_empty_tables(vaddr).retire_into(batch);
        }
        self.retire_backing_into(batch);
    }

    /// Tear this image down as part of a larger operation: its frames, tables
    /// and VA slot join `batch`, whose one shared flush covers every owner
    /// retired into it, and are freed — or quarantined — when it finishes.
    #[cfg(feature = "test-hooks")]
    // reason: only the x86 silent-CPU fixture retires owners into a shared batch.
    #[allow(dead_code)]
    pub(crate) fn retire_into(self, batch: &mut Batch) {
        let mut segments = core::mem::ManuallyDrop::new(self);
        segments.teardown_into(batch);
        #[cfg(feature = "p01-trace")]
        debug_assert!(
            segments.p01_obligation.is_none(),
            "an image with a p01 obligation is retired only by its drop"
        );
        // The batch owns the frames and the slot now; only the metadata is
        // dropped here. An image with no frames releases its slot itself, as
        // the drop does.
        let pages = core::mem::take(&mut segments.pages);
        drop(core::mem::take(&mut segments.writable_pages));
        if pages.is_empty() && segments.va_slot != 0 {
            crate::loader::va_alloc::free_cell_va(segments.va_slot);
        }
    }

    /// Hand every backing frame to `batch` as runs of physically adjacent
    /// frames (the loader allocates them in order), the last one holding the
    /// VA slot until the frames are free.
    fn retire_backing_into(&self, batch: &mut Batch) {
        let mut runs = self.pages.iter().map(|&(_, frame)| frame).peekable();
        while let Some(base) = runs.next() {
            let mut pages = 1;
            while runs.next_if_eq(&(base + pages * PAGE_SIZE)).is_some() {
                pages += 1;
            }
            let run = FrameRun::unpublished(
                base,
                pages,
                Charge::KERNEL,
                RunKind::Leaf,
                KernelAlias::Intact,
            );
            batch.retire(match (runs.peek(), self.va_slot) {
                (None, slot) if slot != 0 => run.with_va_hold(VaHold {
                    release: release_cell_va,
                    start: slot,
                    end: slot,
                }),
                _ => run,
            });
        }
    }
}

/// [`VaHold`] release for a cell image's VA slot.
pub(crate) fn release_cell_va(slot: VAddr, _end: VAddr) {
    crate::loader::va_alloc::free_cell_va(slot);
}

impl Drop for CellSegments {
    fn drop(&mut self) {
        // One batch: leaf and parent invalidations reach every hart once,
        // before either backing or page-table frames return to the allocator.
        // An unconfirmed flush quarantines them, and the VA slot with them.
        let mut batch = Batch::new();
        self.teardown_into(&mut batch);
        #[allow(unused_variables)] // reason: only the p01-trace ledger reads it
        let flushed = batch.complete();
        #[cfg(feature = "p01-trace")]
        if flushed.is_confirmed() {
            crate::p01_resources::finish(self.p01_obligation.take(), 1);
        }
        #[cfg(feature = "p01-trace")]
        if self.p01_obligation.is_some() {
            crate::p01_resources::finish(self.p01_obligation.take(), 6);
        }
        if self.pages.is_empty() && self.va_slot != 0 {
            crate::loader::va_alloc::free_cell_va(self.va_slot);
        }
    }
}
