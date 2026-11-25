//! Physical frame allocator for ViCell kernel.
//!
//! Manages physical memory frames (4KB pages) using a Bitmap Allocator.
//! This allows for O(1) allocation and deallocation (amortized) and frame reuse.

use crate::boot::MemoryMapEntry;
use crate::*;
use core::sync::atomic::{AtomicUsize, Ordering};

// Define PAGE_SIZE to avoid circular dependency with paging.rs
const PAGE_SIZE: usize = 4096;

/// Physical-to-virtual address offset.
/// - RISC-V: 0 (identity-mapped before activate_paging, SATP disabled)
/// - x86_64: HHDM_BASE from Limine (RAM mapped at hhdm+phys, not identity)
static PHYS_OFFSET: AtomicUsize = AtomicUsize::new(0);

/// Set the physical-to-virtual offset. Must be called before `new_from_map`.
/// On x86_64, set to the Limine HHDM base address.
pub fn set_phys_offset(offset: usize) {
    PHYS_OFFSET.store(offset, Ordering::Relaxed);
}

/// Convert a physical address to the virtual address used to access it.
#[inline]
pub fn phys_to_virt(phys: usize) -> usize {
    phys + PHYS_OFFSET.load(Ordering::Relaxed)
}

/// Where the allocation bitmap lives.
pub enum BitmapStorage {
    /// Boot: the bitmap occupies the first frames of the largest managed range, so
    /// the allocator must not hand those frames out. The address is `'static`
    /// because the range is never released.
    Borrowed(&'static mut [u64]),
    /// Tests: the allocator owns the bitmap, so a fixture needs no leaked allocation.
    #[cfg(test)]
    Owned(alloc::boxed::Box<[u64]>),
}

impl BitmapStorage {
    #[inline]
    fn slice(&self) -> &[u64] {
        match self {
            Self::Borrowed(words) => words,
            #[cfg(test)]
            Self::Owned(words) => words,
        }
    }

    #[inline]
    fn slice_mut(&mut self) -> &mut [u64] {
        match self {
            Self::Borrowed(words) => words,
            #[cfg(test)]
            Self::Owned(words) => words,
        }
    }
}

/// Bitmap Frame Allocator
///
/// The index space is the concatenation of the managed ranges, so a hole in the
/// physical map costs no bitmap bits and cannot be allocated: index `i` maps to
/// `ranges[k].start + (i - index_base(k)) * PAGE_SIZE` for the range holding `i`.
pub struct FrameAllocator {
    /// Usable ranges, in address order.
    ranges: [Option<ManagedRange>; MAX_MANAGED_RANGES],
    range_count: usize,
    /// Total frames managed
    total_frames: usize,
    /// Frames whose bitmap bit is currently set.
    used_frames: usize,
    /// Bitmap storage
    bitmap: BitmapStorage,
    /// Index into `ranges` of the range whose first frames hold the bitmap; the
    /// boot heap is reserved in the same range ([`reserve_boot_heap`]).
    bitmap_range: usize,
    /// Index of the last allocated frame (for next-fit search)
    last_alloc_index: usize,
    #[cfg(feature = "p01-trace")]
    p01_epoch: u64,
    #[cfg(feature = "p01-trace")]
    p01_peak: usize,
}

impl FrameAllocator {
    /// Initialize allocator from memory map
    ///
    /// Every usable range below the cell VA slots is managed (see
    /// [`plan_managed_ranges`]); RAM at or above them is logged once here and
    /// left unused. The bitmap is placed at the start of the largest range and
    /// its own frames are marked used.
    pub fn new_from_map(entries: &[MemoryMapEntry]) -> Self {
        let plan = plan_managed_ranges(entries);
        if plan.unmanaged_above() != 0 {
            log::warn!(
                "[boot] allocator: {} MiB of RAM at or above {:#x} left unused (identity-mapped RAM must end below the cell VA slots)",
                plan.unmanaged_above() / (1024 * 1024),
                crate::loader::va_alloc::IDENTITY_RAM_END
            );
        }
        let bitmap_u64_count = plan.total_frames().div_ceil(64);

        // SAFETY: the plan guarantees the largest range can hold the bitmap, and the
        // allocator is constructed once during single-threaded boot.
        let bitmap = unsafe {
            core::slice::from_raw_parts_mut(
                phys_to_virt(plan.bitmap_start()) as *mut u64,
                bitmap_u64_count,
            )
        };
        Self::from_plan(plan, BitmapStorage::Borrowed(bitmap))
    }

    /// The allocator over `plan`: `bitmap` (one bit per managed frame) is cleared
    /// and the frames that hold it, at the start of the plan's bitmap range, are
    /// marked used.
    fn from_plan(plan: ManagedPlan, mut bitmap: BitmapStorage) -> Self {
        bitmap.slice_mut().fill(0);
        let bitmap_start = plan.bitmap_start();
        let bitmap_pages = core::mem::size_of_val(bitmap.slice()).div_ceil(PAGE_SIZE);

        let mut allocator = Self {
            ranges: plan.ranges,
            range_count: plan.range_count(),
            total_frames: plan.total_frames(),
            used_frames: 0,
            bitmap,
            bitmap_range: plan.bitmap_range,
            last_alloc_index: 0,
            #[cfg(feature = "p01-trace")]
            p01_epoch: 1,
            #[cfg(feature = "p01-trace")]
            p01_peak: 0,
        };

        // The bitmap lives inside managed memory, so its own frames are reserved.
        if let Some(index) = allocator.index_of_addr(bitmap_start) {
            for offset in 0..bitmap_pages {
                allocator.mark_used(index + offset);
            }
        }

        allocator
    }

    /// Index of the managed frame at `addr`, or `None` when the address is not managed.
    fn index_of_addr(&self, addr: PhysAddr) -> Option<usize> {
        self.ranges[..self.range_count]
            .iter()
            .filter_map(|range| *range)
            .find(|range| addr >= range.start && addr < range.start + range.frames * PAGE_SIZE)
            .map(|range| range.index_base + (addr - range.start) / PAGE_SIZE)
    }

    /// Does this allocator manage the frame containing `addr`?
    ///
    /// Callers that must not touch firmware/reserved/MMIO memory — the framebuffer
    /// guard, DMA range checks — ask this instead of comparing against one end.
    pub fn manages(&self, addr: PhysAddr) -> bool {
        self.index_of_addr(addr).is_some()
    }

    /// The managed ranges, in address order.
    pub fn managed_ranges(&self) -> impl Iterator<Item = ManagedRange> + '_ {
        self.ranges[..self.range_count]
            .iter()
            .filter_map(|range| *range)
    }

    /// Allocate a physical frame, or `None` on OOM or while the quarantine cap
    /// refuses new allocations ([`Self::quarantine_refuses`]).
    pub fn allocate_frame(&mut self) -> Option<PhysAddr> {
        if self.quarantine_refuses() {
            return None;
        }
        self.allocate_frame_uncapped()
    }

    /// [`Self::allocate_frame`] exempt from the quarantine cap. For page-table
    /// frames only (`memory::paging`): a teardown's normalisation may need one
    /// to restore a freed frame's kernel alias, and boot and the dormant x86
    /// `#PF` demand path panic on `None` — the cap must never turn a
    /// quarantine into a panic or a leaked alias.
    pub fn allocate_frame_uncapped(&mut self) -> Option<PhysAddr> {
        // Simple Next-Fit algorithm
        let start_index = self.last_alloc_index;

        // First pass: from last_alloc to end
        if let Some(idx) = self.find_free(start_index, self.total_frames) {
            self.mark_used(idx);
            self.last_alloc_index = idx + 1;
            return Some(self.frame_index_to_addr(idx));
        }

        // Second pass: from 0 to last_alloc
        if let Some(idx) = self.find_free(0, start_index) {
            self.mark_used(idx);
            self.last_alloc_index = idx + 1;
            return Some(self.frame_index_to_addr(idx));
        }

        None // OOM
    }

    /// The 1/64 quarantine cap (`frame_run::cap_exceeded`): refuse a new
    /// allocation while the frame-run quarantine holds more than 1/64 of the
    /// managed frames behind an unacknowledged invalidation — a degraded
    /// node, never a reused frame. Pin, lease and DMA holds do not count. Each refusal is
    /// counted and logged (rate-limited) by `frame_run`; callers see their
    /// usual out-of-memory result.
    fn quarantine_refuses(&self) -> bool {
        if !crate::memory::frame_run::cap_exceeded(capped_pages(), self.total_frames) {
            return false;
        }
        crate::memory::frame_run::PENDING.note_refused(self.total_frames);
        true
    }

    /// Claim one known-free frame for an in-kernel reuse race.
    ///
    /// Returns `false` when `frame` is outside this allocator or no longer free.
    #[cfg(all(feature = "getrandom-sas-test", target_arch = "riscv64"))]
    pub(crate) fn claim_exact_frame_for_test(&mut self, frame: PhysAddr) -> bool {
        let Some(index) = self.addr_to_frame_index(frame) else {
            return false;
        };
        if self.is_frame_allocated(index) {
            return false;
        }
        self.mark_used(index);
        self.last_alloc_index = index + 1;
        true
    }

    /// Deallocate a physical frame
    pub fn deallocate_frame(&mut self, frame: PhysAddr) {
        if let Some(idx) = self.addr_to_frame_index(frame) {
            if !self.mark_free(idx) {
                log::warn!("Attempted to free unused frame: 0x{:X}", frame);
            }
            // Optimization: Reset last_alloc_index if we freed a lower index?
            // Maybe not needed for next-fit.
        } else {
            log::warn!("Attempted to free invalid frame: 0x{:X}", frame);
        }
    }

    #[cfg(any(feature = "p01-trace", feature = "test-hooks"))]
    pub(crate) fn p01_allocated(&self, frame: PhysAddr) -> bool {
        frame.is_multiple_of(PAGE_SIZE)
            && self
                .addr_to_frame_index(frame)
                .is_some_and(|index| self.is_frame_allocated(index))
    }

    // --- Helper bits ---

    fn find_free(&self, start_idx: usize, end_idx: usize) -> Option<usize> {
        let mut bit_idx = start_idx;
        while bit_idx < end_idx {
            let u64_idx = bit_idx / 64;
            let bit_offset = bit_idx % 64;

            let block = self.bitmap.slice()[u64_idx];

            if block == !0 {
                bit_idx = (u64_idx + 1) * 64;
                continue;
            }

            let free_mask = !block & (!0u64 << bit_offset);
            if free_mask != 0 {
                let first_free = u64_idx * 64 + free_mask.trailing_zeros() as usize;
                return if first_free < end_idx {
                    Some(first_free)
                } else {
                    None
                };
            }
            bit_idx = (u64_idx + 1) * 64;
        }
        None
    }

    fn mark_used(&mut self, idx: usize) -> bool {
        let u64_idx = idx / 64;
        let bit_offset = idx % 64;
        let mask = 1u64 << bit_offset;
        if self.bitmap.slice()[u64_idx] & mask != 0 {
            return false;
        }
        #[cfg(feature = "p01-trace")]
        let p01_physical = crate::p01_retained::physical_write();
        self.bitmap.slice_mut()[u64_idx] |= mask;
        self.used_frames += 1;
        #[cfg(feature = "p01-trace")]
        if let Some(write) = p01_physical.as_ref() {
            self.p01_transition();
            crate::p01_retained::physical_allocated(
                write,
                self.frame_index_to_addr(idx),
                self.p01_epoch,
            );
        }
        true
    }

    fn mark_free(&mut self, idx: usize) -> bool {
        let u64_idx = idx / 64;
        let bit_offset = idx % 64;
        let mask = 1u64 << bit_offset;
        if self.bitmap.slice()[u64_idx] & mask == 0 {
            return false;
        }
        #[cfg(feature = "p01-trace")]
        let p01_physical = crate::p01_retained::physical_write();
        self.bitmap.slice_mut()[u64_idx] &= !mask;
        self.used_frames -= 1;
        #[cfg(feature = "p01-trace")]
        if let Some(write) = p01_physical.as_ref() {
            self.p01_transition();
            crate::p01_retained::physical_freed(
                write,
                self.frame_index_to_addr(idx),
                self.p01_epoch,
            );
        }
        true
    }

    #[cfg(feature = "p01-trace")]
    fn p01_transition(&mut self) {
        // The enclosing PhysicalWrite already owns admission through the bitmap
        // transition. A second admission could fail after the actual mutation,
        // leaving an invented/repeated source epoch in the physical inventory.
        if let Some(next) = self.p01_epoch.checked_add(1) {
            self.p01_epoch = next;
            self.p01_peak = self.p01_peak.max(self.used_frames);
        } else {
            crate::p01_retained::physical_fault();
        }
    }

    /// Arm-time exact bitmap population, including bitmap backing reservations.
    /// Caller holds FRAME_ALLOCATOR and keeps admission closed through the census.
    /// This records unknown births, not synthetic allocations at the census epoch.
    #[cfg(feature = "p01-trace")]
    pub(crate) fn p01_seed_physical(&self) {
        let mut counted = 0usize;
        for (word_index, word) in self.bitmap.slice().iter().copied().enumerate() {
            let mut used = word;
            while used != 0 {
                let bit = used.trailing_zeros() as usize;
                let index = word_index * 64 + bit;
                if index >= self.total_frames {
                    crate::p01_retained::physical_fault();
                } else {
                    crate::p01_retained::physical_preexisting(
                        self.frame_index_to_addr(index),
                        self.p01_epoch,
                    );
                    counted += 1;
                }
                used &= used - 1;
            }
        }
        if counted != self.used_frames {
            crate::p01_retained::physical_fault();
        }
    }

    #[cfg(feature = "p01-trace")]
    pub(crate) fn p01_snapshot(&self) -> (u64, usize) {
        (self.p01_epoch, self.p01_peak)
    }

    #[cfg(feature = "p01-trace")]
    pub(crate) fn p01_reset_peak(&mut self) {
        if let Some(_observation) = crate::p01_measurement::observation() {
            self.p01_peak = self.used_frames;
        }
    }

    fn frame_index_to_addr(&self, idx: usize) -> PhysAddr {
        for range in self.ranges[..self.range_count]
            .iter()
            .filter_map(|range| *range)
        {
            if idx >= range.index_base && idx < range.index_base + range.frames {
                return range.start + (idx - range.index_base) * PAGE_SIZE;
            }
        }
        // Callers only pass indices below `total_frames`, which the ranges cover.
        panic!("frame index {idx} is outside the managed ranges");
    }

    fn addr_to_frame_index(&self, addr: PhysAddr) -> Option<usize> {
        self.index_of_addr(addr)
    }

    /// Get total available memory in bytes
    pub fn total_memory(&self) -> usize {
        self.total_frames * PAGE_SIZE
    }

    /// Get allocator-committed memory in bytes.
    pub fn used_memory(&self) -> usize {
        self.used_frames * PAGE_SIZE
    }

    // ── Snapshot serialization accessors ──────────────────────────────────────

    /// Physical start address of the **first** managed range.
    ///
    /// The allocator's memory is a list of ranges: callers that walk physical memory
    /// (the snapshot serializer) must iterate [`Self::managed_ranges`] instead of
    /// assuming `start..end` is contiguous.
    pub fn memory_start(&self) -> PhysAddr {
        self.managed_ranges()
            .next()
            .map(|range| range.start)
            .unwrap_or(0)
    }

    /// Physical end address (exclusive) of the allocator's managed region.
    /// Physical end address of the **last** managed range.
    ///
    /// `memory_start()..memory_end()` is only a complete description of managed
    /// memory when there is a single range; with a hole it spans unmanaged memory.
    pub fn memory_end(&self) -> PhysAddr {
        self.managed_ranges()
            .last()
            .map(|range| range.start + range.frames * PAGE_SIZE)
            .unwrap_or(0)
    }

    /// Total number of 4096-byte frames managed by this allocator.
    pub fn total_frames(&self) -> usize {
        self.total_frames
    }

    /// Number of frames whose allocation bitmap bit is set.
    pub fn used_frames(&self) -> usize {
        self.used_frames
    }

    /// Number of frames currently available for allocation.
    pub fn free_frames(&self) -> usize {
        self.total_frames - self.used_frames
    }

    /// Test-hooks: a copy of this allocator's allocation bitmap.
    ///
    /// A ledger mismatch is diagnosed by *which* frames moved, not only by how
    /// many: a page table, a segment frame and a stack frame are all one 4 KiB
    /// frame, and only the address (plus the page tables' own view of it) says
    /// which of them was lost.
    #[cfg(feature = "test-hooks")]
    pub fn used_bitmap(&self) -> alloc::vec::Vec<u64> {
        self.bitmap.slice().to_vec()
    }

    /// Physical frame size used by this allocator.
    pub const fn page_size(&self) -> usize {
        PAGE_SIZE
    }
    /// Byte range (start, end) occupied by the frame allocator bitmap storage.
    pub fn bitmap_range(&self) -> (usize, usize) {
        let start = self.bitmap.slice().as_ptr() as usize;
        let end = start + core::mem::size_of_val(self.bitmap.slice());
        (start, (end + PAGE_SIZE - 1) & !(PAGE_SIZE - 1))
    }
    /// Returns `true` if frame `idx` is currently allocated (in use).
    ///
    /// Used by the snapshot serializer to enumerate only the allocated frames,
    /// avoiding snapshotting free memory and reducing snapshot size.
    pub fn is_frame_allocated(&self, idx: usize) -> bool {
        if idx >= self.total_frames {
            return false;
        }
        let u64_idx = idx / 64;
        let bit_offset = idx % 64;
        (self.bitmap.slice()[u64_idx] >> bit_offset) & 1 != 0
    }

    /// Physical address of frame `idx`.
    pub fn frame_addr(&self, idx: usize) -> PhysAddr {
        self.frame_index_to_addr(idx)
    }

    /// Mark `n` consecutive frames starting at `start_idx` as allocated.
    ///
    /// Used by `allocate_guest_ram` after locating a free run so the final
    /// allocation is a single, atomic lock hold rather than n individual calls.
    pub fn mark_range_used(&mut self, start_idx: usize, n: usize) {
        debug_assert!(
            start_idx + n <= self.total_frames,
            "mark_range_used: frame range [{}, {}) out of bounds (total={})",
            start_idx,
            start_idx + n,
            self.total_frames,
        );
        for i in 0..n {
            self.mark_used(start_idx + i);
        }
    }

    /// Index of the first run of `n` free frames inside one managed range whose
    /// first frame's address is a multiple of `align_bytes` (a power of two, at
    /// least a page). Linear: an allocated frame moves the candidate start past
    /// it, to the next aligned frame, so no frame is read twice. A run never
    /// crosses from one range into the next: index space concatenates the
    /// ranges, so consecutive indices on either side of a range boundary are
    /// not consecutive frames.
    pub fn find_free_run_aligned(&self, n: usize, align_bytes: usize) -> Option<usize> {
        let align = align_bytes / PAGE_SIZE;
        if n == 0 || align == 0 || !align.is_power_of_two() {
            return None;
        }
        for range in self.ranges[..self.range_count].iter().flatten() {
            let first_pfn = range.start / PAGE_SIZE;
            let end = range.index_base + range.frames;
            // The first index at or after `index` whose frame is aligned.
            let aligned_from = |index: usize| {
                let pfn = first_pfn + (index - range.index_base);
                index + (pfn.next_multiple_of(align) - pfn)
            };
            let mut start = aligned_from(range.index_base);
            let mut index = start;
            while index < end && index - start < n {
                if self.is_frame_allocated(index) {
                    start = aligned_from(index + 1);
                    index = start;
                } else {
                    index += 1;
                }
            }
            if index <= end && index - start == n {
                return Some(start);
            }
        }
        None
    }

    /// Longest run of free frames the allocator currently has.
    ///
    /// A failed guest-RAM carve reports this: "no contiguous run" without the
    /// size of the largest one cannot tell a fragmented map from a small one.
    pub fn largest_free_run(&self) -> usize {
        let mut best = 0usize;
        let mut run = 0usize;
        for idx in 0..self.total_frames {
            if self.is_frame_allocated(idx) {
                run = 0;
            } else {
                run += 1;
                best = best.max(run);
            }
        }
        best
    }

    /// Find `n` consecutive free frames inside **one** managed range and mark them
    /// all allocated.
    ///
    /// Returns the physical address of the first frame, or `None` when no range has
    /// a free run of `n` frames. A run never straddles a hole between two ranges —
    /// the frames it would cover are not managed by this allocator.
    pub fn allocate_contiguous(&mut self, n: usize) -> Option<PhysAddr> {
        if n == 1 {
            return self.allocate_frame();
        }
        if self.quarantine_refuses() {
            return None;
        }
        (0..self.range_count).find_map(|range_index| self.allocate_contiguous_in(range_index, n))
    }

    /// First run of `n` free frames inside managed range `range_index`, marked
    /// allocated; `None` when that range has no such run.
    fn allocate_contiguous_in(&mut self, range_index: usize, n: usize) -> Option<PhysAddr> {
        // No allocation here: this runs during boot (the heap reservation) before the
        // global allocator exists.
        let range = self.ranges[range_index]?;
        if range.frames < n {
            return None;
        }
        let limit = range.index_base + range.frames - n;
        'outer: for start in range.index_base..=limit {
            for i in 0..n {
                if self.is_frame_allocated(start + i) {
                    continue 'outer;
                }
            }
            for i in 0..n {
                self.mark_used(start + i);
            }
            return Some(self.frame_index_to_addr(start));
        }
        None
    }

    /// Allocate `n` contiguous frames whose base address is `align_bytes`-aligned.
    ///
    /// Needed by hardware structures with alignment > one frame — e.g. the
    /// Stage-2 concatenated root (VTTBR_EL2.BADDR requires 8 KB alignment).
    /// `allocate_contiguous` returns the FIRST free run, whose parity depends
    /// on every allocation made since boot — relying on it for alignment is a
    /// latent misalignment bug, not a guarantee.
    ///
    /// `align_bytes` must be a power of two ≥ PAGE_SIZE.
    pub fn allocate_contiguous_aligned(
        &mut self,
        n: usize,
        align_bytes: usize,
    ) -> Option<PhysAddr> {
        debug_assert!(align_bytes.is_power_of_two() && align_bytes >= PAGE_SIZE);
        if self.quarantine_refuses() {
            return None;
        }
        // No allocation here: see `allocate_contiguous`.
        for range_index in 0..self.range_count {
            let Some(range) = self.ranges[range_index] else {
                continue;
            };
            if range.frames < n {
                continue;
            }
            let limit = range.index_base + range.frames - n;
            'outer: for start in range.index_base..=limit {
                if !self.frame_index_to_addr(start).is_multiple_of(align_bytes) {
                    continue;
                }
                for i in 0..n {
                    if self.is_frame_allocated(start + i) {
                        continue 'outer;
                    }
                }
                for i in 0..n {
                    self.mark_used(start + i);
                }
                return Some(self.frame_index_to_addr(start));
            }
        }
        None
    }
}

/// The frame allocator. Private: every use goes through [`lock`], [`with`] or
/// one of the single-operation accessors below, so no code outside this file
/// can hold it across a shared TLB flush (see [`FlushCap`]).
static FRAME_ALLOCATOR: crate::sync::Spinlock<Option<FrameAllocator>> =
    crate::sync::Spinlock::ranked(None, crate::sync::rank::FRAME_ALLOCATOR);

/// Per-hart: this hart holds [`FRAME_ALLOCATOR`]. The lock masks interrupts, so
/// a holder cannot migrate and its hart id is stable for the whole hold.
#[cfg(not(test))]
static HELD_HERE: [core::sync::atomic::AtomicBool; crate::sched::smp::MAX_HARTS] =
    [const { core::sync::atomic::AtomicBool::new(false) }; crate::sched::smp::MAX_HARTS];

#[cfg(not(test))]
fn held_slot() -> Option<&'static core::sync::atomic::AtomicBool> {
    // An AP before its hart-local install has no id and is not tracked; it
    // allocates nothing then.
    crate::sync::installed_hart_id().and_then(|hart| HELD_HERE.get(hart))
}

#[cfg(not(test))]
fn set_held(held: bool) {
    if let Some(slot) = held_slot() {
        slot.store(held, Ordering::Relaxed);
    }
}

/// Does this hart hold the frame allocator right now?
#[cfg(not(test))]
pub fn held_here() -> bool {
    held_slot().is_some_and(|slot| slot.load(Ordering::Relaxed))
}

// Host tests share no hart: one flag per thread.
#[cfg(test)]
mod held_by_thread {
    extern crate std;
    std::thread_local!(pub(super) static HELD_HERE: core::cell::Cell<bool> = const { core::cell::Cell::new(false) });
}

#[cfg(test)]
fn set_held(held: bool) {
    held_by_thread::HELD_HERE.with(|slot| slot.set(held));
}

#[cfg(test)]
pub fn held_here() -> bool {
    held_by_thread::HELD_HERE.with(core::cell::Cell::get)
}

/// Pages the frame-run quarantine holds behind an invalidation now: the cap's
/// input.
#[cfg(not(test))]
fn capped_pages() -> usize {
    crate::memory::frame_run::PENDING
        .counters()
        .awaiting_invalidation()
}

// Host tests: one count per thread, set by the test itself, so no allocator
// test sees another test's quarantine.
#[cfg(test)]
pub(crate) mod quarantine_by_thread {
    extern crate std;
    std::thread_local!(pub(crate) static PAGES: core::cell::Cell<usize> = const { core::cell::Cell::new(0) });
}

#[cfg(test)]
fn capped_pages() -> usize {
    quarantine_by_thread::PAGES.with(core::cell::Cell::get)
}

/// The right to issue a shared (kernel-root) TLB flush.
///
/// Every shared flush takes `&mut FlushCap`, and [`lock`] borrows the same cap
/// for as long as its [`AllocGuard`] lives, so "flush while holding the frame
/// allocator" is a borrow error (E0499) rather than a comment. [`with`] takes no
/// cap but only a `Send` closure, and the cap is `!Send`, so the closure cannot
/// capture one (E0277). [`FlushCap::take`] refuses to mint a cap on a hart that
/// already holds the allocator, so none can be made inside either.
///
/// Zero-sized; minting one is free. `scripts/check-frame-guard-compile-fail.sh`
/// keeps the compile-time half of this contract honest.
#[must_use]
pub struct FlushCap {
    _not_send: core::marker::PhantomData<*mut ()>,
}

impl FlushCap {
    /// A cap for this hart, or `None` while it holds the frame allocator.
    pub fn take() -> Option<FlushCap> {
        (!held_here()).then_some(FlushCap {
            _not_send: core::marker::PhantomData,
        })
    }
}

/// The locked allocator, borrowing the caller's [`FlushCap`] for its lifetime.
pub struct AllocGuard<'c> {
    guard: crate::sync::SpinlockGuard<'static, Option<FrameAllocator>>,
    _cap: core::marker::PhantomData<&'c mut FlushCap>,
}

impl core::ops::Deref for AllocGuard<'_> {
    type Target = Option<FrameAllocator>;
    fn deref(&self) -> &Self::Target {
        &self.guard
    }
}

impl core::ops::DerefMut for AllocGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.guard
    }
}

impl Drop for AllocGuard<'_> {
    fn drop(&mut self) {
        // Cleared before the field drop releases the lock (and interrupts).
        set_held(false);
    }
}

/// Lock the frame allocator for a multi-step hold. `cap` stays borrowed until
/// the guard drops, so no shared flush can be issued meanwhile.
pub fn lock(cap: &mut FlushCap) -> AllocGuard<'_> {
    let _ = cap;
    let guard = FRAME_ALLOCATOR.lock();
    set_held(true);
    AllocGuard {
        guard,
        _cap: core::marker::PhantomData,
    }
}

/// Run `f` on the allocator, or return `None` before it is installed.
///
/// `f` must be `Send`, so it cannot capture a [`FlushCap`]; a cap cannot be
/// minted inside it either. Neither can anything it calls.
pub fn with<R>(f: impl FnOnce(&mut FrameAllocator) -> R + Send) -> Option<R> {
    let mut guard = FRAME_ALLOCATOR.lock();
    set_held(true);
    let result = guard.as_mut().map(f);
    set_held(false);
    drop(guard);
    result
}

/// Counts of the installed allocator, in frames.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameStats {
    pub total_frames: usize,
    pub used_frames: usize,
    pub free_frames: usize,
}

/// The allocator's counts, or `None` before it is installed.
pub fn stats() -> Option<FrameStats> {
    with(|allocator| FrameStats {
        total_frames: allocator.total_frames(),
        used_frames: allocator.used_frames(),
        free_frames: allocator.free_frames(),
    })
}

/// One frame, or `None` (no allocator, out of memory, or quarantine cap).
pub fn allocate_frame() -> Option<PhysAddr> {
    with(FrameAllocator::allocate_frame).flatten()
}

/// `n` contiguous frames in one managed range.
pub fn allocate_contiguous(n: usize) -> Option<PhysAddr> {
    with(|allocator| allocator.allocate_contiguous(n)).flatten()
}

/// `n` contiguous frames whose base is `align_bytes`-aligned.
pub fn allocate_contiguous_aligned(n: usize, align_bytes: usize) -> Option<PhysAddr> {
    with(|allocator| allocator.allocate_contiguous_aligned(n, align_bytes)).flatten()
}

/// Return one frame. Ignored before the allocator is installed.
pub fn deallocate_frame(frame: PhysAddr) {
    let _ = with(|allocator| allocator.deallocate_frame(frame));
}

/// Return `pages` frames starting at `base`.
pub fn deallocate_frames(base: PhysAddr, pages: usize) {
    let _ = with(|allocator| {
        for page in 0..pages {
            allocator.deallocate_frame(base + page * PAGE_SIZE);
        }
    });
}

/// Publish the boot allocator.
///
/// # Safety
/// Boot only, once, before any other accessor: the lock's previous value is
/// overwritten without being dropped (it may not be initialised memory yet on
/// the arches that publish before their own paging is live).
pub unsafe fn install(allocator: FrameAllocator) {
    // SAFETY: the caller guarantees boot-time, single-hart, single call.
    unsafe { core::ptr::write(&mut *FRAME_ALLOCATOR.lock(), Some(allocator)) };
}

/// Swap the installed allocator (host tests and test-hooks fixtures that need
/// a missing or synthetic one); returns the previous one.
#[cfg(any(test, feature = "test-hooks"))]
pub(crate) fn replace(allocator: Option<FrameAllocator>) -> Option<FrameAllocator> {
    core::mem::replace(&mut *FRAME_ALLOCATOR.lock(), allocator)
}

/// A non-copyable allocation lease returned to the bitmap only on drop, with
/// its charge refunded then.
pub struct OwnedFrame {
    physical: PhysAddr,
    charge: super::frame_run::RunCharge,
    #[cfg(feature = "p01-trace")]
    p01_retained: Option<crate::p01_retained::Token>,
}

impl OwnedFrame {
    /// One frame billed to `charge`'s memory account (the kernel is never
    /// billed). The charge is taken first: when it does not fit, no frame is
    /// taken.
    pub(crate) fn allocate(charge: super::cell_quota::Charge) -> Option<Self> {
        let reservation = super::cell_quota::Reservation::take(
            charge,
            super::cell_quota::Account::Memory,
            super::paging::PAGE_SIZE,
        )
        .ok()?;
        let physical = allocate_frame()?;
        Some(Self {
            physical,
            charge: super::frame_run::RunCharge::of(reservation.into(), 1),
            #[cfg(feature = "p01-trace")]
            p01_retained: None,
        })
    }

    pub const fn physical_address(&self) -> PhysAddr {
        self.physical
    }

    #[cfg(feature = "p01-trace")]
    pub(crate) fn p01_retain(
        &mut self,
        owner: Option<crate::p01_measurement::Owner>,
        reason: &str,
    ) {
        if self.p01_retained.is_some() {
            crate::p01_retained::fault();
            return;
        }
        let provenance = crate::p01_retained::Reason::deferred(reason);
        if owner.is_some() && provenance.is_none() {
            crate::p01_retained::fault();
        }
        self.p01_retained = crate::p01_retained::begin(
            owner,
            self.physical,
            1,
            provenance.unwrap_or(crate::p01_retained::Reason::Deferred),
        );
    }

    /// Hand the frame, and its charge, to a
    /// [`FrameRun`](super::frame_run::FrameRun) that the frame-run quarantine
    /// frees once the invalidation covering it is confirmed. The caller already
    /// removed it from its private root, so the run is `Unpublished`; the
    /// kernel's own alias of the frame was never removed (`Intact`).
    pub(crate) fn into_run(self, kind: super::frame_run::RunKind) -> super::frame_run::FrameRun {
        let frame = core::mem::ManuallyDrop::new(self);
        let run = super::frame_run::FrameRun::unpublished_carrying(
            frame.physical,
            1,
            frame.charge,
            kind,
            super::frame_run::KernelAlias::Intact,
        );
        #[cfg(feature = "p01-trace")]
        return run.with_p01_retained(frame.p01_retained);
        #[cfg(not(feature = "p01-trace"))]
        run
    }
}

impl Drop for OwnedFrame {
    fn drop(&mut self) {
        #[cfg(feature = "p01-trace")]
        let _range_write = crate::p01_retained::release_write();
        if let Some(frames) = FRAME_ALLOCATOR.lock().as_mut() {
            #[cfg(feature = "p01-trace")]
            let released = frames.p01_allocated(self.physical);
            frames.deallocate_frame(self.physical);
            self.charge.refund(1);
            #[cfg(feature = "p01-trace")]
            if released {
                crate::p01_retained::retire(self.p01_retained.take());
            } else if self.p01_retained.is_some() {
                crate::p01_retained::fault();
            }
        }
        #[cfg(feature = "p01-trace")]
        if self.p01_retained.is_some() {
            crate::p01_retained::fault();
        }
    }
}

/// Reserve the boot heap as **one contiguous run** of `frames` 4 KiB frames.
///
/// The heap is initialized at the first frame's physical address and covers
/// `frames * 4096` bytes, so the frames must actually be adjacent. Allocating them
/// one at a time and trusting the allocator's next-fit cursor only works while the
/// free frames happen to be linear: on a fragmented map it reserves scattered
/// frames and then writes a heap over memory it does not own. `None` means no run of
/// that length exists, and the caller must fail boot rather than continue with a
/// smaller or scattered heap.
pub fn reserve_contiguous_run(allocator: &mut FrameAllocator, frames: usize) -> Option<PhysAddr> {
    allocator.allocate_contiguous(frames)
}

/// Give back a run [`reserve_contiguous_run`] reserved and the caller never
/// published: nothing ever mapped it outside the kernel's own alias, so no
/// invalidation is owed and the frames return in the caller's allocator hold.
pub fn unreserve_contiguous_run(allocator: &mut FrameAllocator, base: PhysAddr, frames: usize) {
    for frame in 0..frames {
        allocator.deallocate_frame(base + frame * PAGE_SIZE);
    }
}

/// Reserve the boot heap: the first run of `frames` free frames inside the range
/// that hosts the frame bitmap, as one contiguous transaction like
/// [`reserve_contiguous_run`].
///
/// The bitmap occupies the first frames of that range, and every frame handed out
/// before the heap comes from the next-fit cursor starting at the first managed
/// frame, so the frames of that range below the heap are exactly the bitmap plus
/// the early boot frames that reached it. `main.rs` registers
/// `[bitmap start, heap start)` as shared kernel memory and relies on this. A first
/// fit over every range does not keep it: when the largest range (the bitmap's) is
/// not the first one with room — UEFI maps at ≥ 1 GiB under VMware and at 4 GiB under
/// OVMF — the heap landed below the bitmap and that registration failed.
///
/// `None` (no such run in the bitmap's range) must fail boot; placing the heap in
/// another range would break the registered span.
pub fn reserve_boot_heap(allocator: &mut FrameAllocator, frames: usize) -> Option<PhysAddr> {
    allocator.allocate_contiguous_in(allocator.bitmap_range, frames)
}

/// Largest number of disjoint usable ranges the allocator tracks.
///
/// Every boot path hands the allocator a converted map of at most
/// [`crate::boot::MAX_MEMORY_MAP_ENTRIES`] entries, so this covers any map the
/// kernel accepts. Firmware maps are not small: under OVMF, Limine reports more
/// than eight disjoint usable ranges, and real UEFI machines often report twenty
/// or more. More than this halts rather than silently dropping memory.
pub const MAX_MANAGED_RANGES: usize = crate::boot::MAX_MEMORY_MAP_ENTRIES;

/// One contiguous, page-aligned run of usable physical memory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ManagedRange {
    /// First frame address of the run.
    pub start: PhysAddr,
    /// Frames in the run (always ≥ 1).
    pub frames: usize,
    /// Global index of this run's first frame (prefix sum of the earlier runs).
    index_base: usize,
}

/// Normalized view of the memory map: every usable frame the allocator manages.
///
/// The allocator's index space is the concatenation of `ranges`, so a hole between
/// two ranges costs no bitmap bits and no allocation can land in it — the reason
/// the managed range is *not* simply widened to the highest address.
pub struct ManagedPlan {
    ranges: [Option<ManagedRange>; MAX_MANAGED_RANGES],
    count: usize,
    bitmap_range: usize,
    total_frames: usize,
    /// Usable bytes at or above the plan's ceiling, left unmanaged.
    unmanaged_above: usize,
}

impl ManagedPlan {
    /// The ranges, in address order.
    pub fn ranges(&self) -> impl Iterator<Item = ManagedRange> + '_ {
        self.ranges[..self.count].iter().filter_map(|range| *range)
    }

    pub fn range_count(&self) -> usize {
        self.count
    }

    pub fn total_frames(&self) -> usize {
        self.total_frames
    }

    /// Usable bytes at or above the ceiling, left unmanaged.
    pub fn unmanaged_above(&self) -> usize {
        self.unmanaged_above
    }

    /// Physical address of the range that hosts the bitmap.
    pub fn bitmap_start(&self) -> PhysAddr {
        self.ranges[self.bitmap_range]
            .expect("the bitmap range is always present")
            .start
    }
}

/// The page-aligned frames `[start, end)` a usable entry contributes, without
/// physical frame 0; `None` for any other entry or one smaller than a page.
fn usable_frames(entry: &MemoryMapEntry) -> Option<(PhysAddr, PhysAddr)> {
    if entry.ty != crate::boot::MemoryType::Usable || entry.length == 0 {
        return None;
    }
    let start = (entry.base + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
    let end = (entry.base + entry.length) & !(PAGE_SIZE - 1);
    // Physical frame 0 (0x0..PAGE_SIZE) is never managed: on x86 it contains
    // real-mode IVT/BDA and physical 0 is the universal 'not set' sentinel for
    // VI_KERNEL_CR3; on all architectures address 0 is the null/unmapped sentinel.
    let start = start.max(PAGE_SIZE);
    (end > start).then_some((start, end))
}

/// Normalize a memory map into the set of usable ranges the allocator manages:
/// every usable frame below [`IDENTITY_RAM_END`](crate::loader::va_alloc::IDENTITY_RAM_END),
/// the first cell VA slot. Grants and SHM pages keep their physical address as
/// VA, and RV64/AArch64 map all RAM there, so RAM at or above it would alias
/// the cell slots or the stack arena; it is left unused and counted in
/// [`ManagedPlan::unmanaged_above`], and the machine boots with the rest.
///
/// Malformed authoritative input (an entry that overlaps another, a usable entry
/// smaller than a page, or more ranges than [`MAX_MANAGED_RANGES`]) **halts**: a
/// wrong guess here is memory corruption, not a degraded boot.
pub fn plan_managed_ranges(entries: &[MemoryMapEntry]) -> ManagedPlan {
    plan_managed_ranges_below(entries, crate::loader::va_alloc::IDENTITY_RAM_END)
}

/// [`plan_managed_ranges`] with an explicit `ceiling` (rounded down to a
/// page): usable RAM at or above it is not managed.
pub fn plan_managed_ranges_below(entries: &[MemoryMapEntry], ceiling: PhysAddr) -> ManagedPlan {
    let ceiling = ceiling & !(PAGE_SIZE - 1);
    let mut ranges: [Option<ManagedRange>; MAX_MANAGED_RANGES] = [None; MAX_MANAGED_RANGES];
    let mut count = 0usize;
    let mut total_frames = 0usize;
    let below = |entry: &MemoryMapEntry| {
        usable_frames(entry)
            .map(|(start, end)| (start, end.min(ceiling)))
            .filter(|(start, end)| end > start)
    };

    // This runs before the heap exists, so the plan is built without allocating:
    // repeatedly select the lowest usable run at or after `cursor`.
    let mut cursor: PhysAddr = 0;
    loop {
        let mut best: Option<(PhysAddr, PhysAddr)> = None;
        for (start, end) in entries.iter().filter_map(below) {
            if start < cursor {
                continue;
            }
            if best.is_none_or(|(best_start, _)| start < best_start) {
                best = Some((start, end));
            }
        }
        let Some((start, end)) = best else {
            break;
        };
        let frames = (end - start) / PAGE_SIZE;
        let previous = ranges[..count]
            .iter_mut()
            .filter_map(|range| range.as_mut())
            .next_back();
        match previous {
            // Adjacent usable entries are one range.
            Some(last) if start == last.start + last.frames * PAGE_SIZE => {
                last.frames += frames;
            }
            _ => {
                assert!(
                    count < MAX_MANAGED_RANGES,
                    "memory map has more usable ranges than the allocator tracks \
                     ({} > {MAX_MANAGED_RANGES})",
                    count + 1
                );
                ranges[count] = Some(ManagedRange {
                    start,
                    frames,
                    index_base: total_frames,
                });
                count += 1;
            }
        }
        total_frames += frames;
        cursor = end;
    }

    // Every usable entry must be fully inside exactly one selected range: an entry
    // that overlaps another cannot be resolved by guessing which claim wins.
    for (start, end) in entries.iter().filter_map(below) {
        let contained = ranges[..count]
            .iter()
            .filter_map(|range| *range)
            .any(|range| {
                let range_end = range.start + range.frames * PAGE_SIZE;
                start >= range.start && end <= range_end
            });
        if !contained {
            panic!(
                "overlapping usable memory map entries: {start:#x}..{end:#x} is not \
                 contained in any managed range"
            );
        }
    }
    let unmanaged_above = entries
        .iter()
        .filter_map(usable_frames)
        .map(|(start, end)| end.saturating_sub(start.max(ceiling)))
        .sum();

    // The bitmap lives in the largest range: it needs `total_frames / 512` bytes and
    // any range holding at least one frame is at least one page, so the largest range
    // always has room.
    let bitmap_range = ranges[..count]
        .iter()
        .filter_map(|range| *range)
        .enumerate()
        .max_by_key(|(_, range)| range.frames)
        .map(|(index, _)| index)
        .unwrap_or(0);

    ManagedPlan {
        ranges,
        count,
        bitmap_range,
        total_frames,
        unmanaged_above,
    }
}

/// A `FrameAllocator` over synthetic managed ranges, for host tests that need
/// the real allocation path without a boot memory map.
///
/// Test-only: the boot path constructs the allocator from the firmware map
/// ([`FrameAllocator::new_from_map`]).
#[cfg(test)]
pub(crate) fn allocator_for_tests(ranges: &[(usize, usize)]) -> FrameAllocator {
    let mut slots: [Option<ManagedRange>; MAX_MANAGED_RANGES] = [None; MAX_MANAGED_RANGES];
    let mut total = 0usize;
    for (index, (start, frames)) in ranges.iter().enumerate() {
        slots[index] = Some(ManagedRange {
            start: *start,
            frames: *frames,
            index_base: total,
        });
        total += *frames;
    }
    FrameAllocator {
        ranges: slots,
        range_count: ranges.len(),
        total_frames: total,
        used_frames: 0,
        bitmap: BitmapStorage::Owned(alloc::vec![0u64; total.div_ceil(64)].into_boxed_slice()),
        // The owned bitmap is outside managed memory; the boot heap would go in range 0.
        bitmap_range: 0,
        last_alloc_index: 0,
        #[cfg(feature = "p01-trace")]
        p01_epoch: 1,
        #[cfg(feature = "p01-trace")]
        p01_peak: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        plan_managed_ranges, plan_managed_ranges_below, reserve_boot_heap, reserve_contiguous_run,
        FrameAllocator, PAGE_SIZE,
    };
    use crate::boot::{MemoryMapEntry, MemoryType};

    #[test]
    fn no_flush_cap_is_minted_while_this_hart_holds_the_allocator() {
        let _serial = crate::TEST_STATE_LOCK.lock();
        let previous = super::replace(Some(super::allocator_for_tests(&[(0x10_0000, 8)])));

        let mut cap = super::FlushCap::take().expect("nothing held yet");
        {
            let mut guard = super::lock(&mut cap);
            assert!(super::held_here());
            assert!(
                super::FlushCap::take().is_none(),
                "no second cap under the guard"
            );
            assert!(guard
                .as_mut()
                .and_then(FrameAllocator::allocate_frame)
                .is_some());
        }
        assert!(!super::held_here(), "the guard's drop clears the hold");

        let inside = super::with(|_| (super::held_here(), super::FlushCap::take().is_none()));
        assert_eq!(
            inside,
            Some((true, true)),
            "with() holds it and refuses a cap"
        );
        assert!(!super::held_here());
        assert!(super::FlushCap::take().is_some());

        assert_eq!(super::stats().map(|stats| stats.used_frames), Some(1));
        super::replace(previous);
    }

    /// A domain's table frame (`OwnedFrame::allocate`, the only allocation a
    /// private root's growth makes) over its owner's limit is refused before
    /// the allocator is asked; one that fits is billed until it is freed.
    #[test]
    fn a_domain_table_frame_over_the_limit_is_refused_before_a_frame_is_taken() {
        use crate::memory::cell_quota::{self, Charge, Limits};
        let _serial = crate::TEST_STATE_LOCK.lock();
        let previous = super::replace(Some(super::allocator_for_tests(&[(0x10_0000, 8)])));
        let cell = types::CellId((cell_quota::MAX_CELLS - 3) as u64);
        let used = || super::stats().map(|stats| stats.used_frames);

        cell_quota::register(
            cell,
            4,
            Limits {
                memory: PAGE_SIZE - 1,
                guest: 0,
            },
        );
        assert!(super::OwnedFrame::allocate(Charge::new(cell, 4)).is_none());
        assert_eq!(used(), Some(0), "no frame was taken");
        assert_eq!(cell_quota::in_use(cell), 0);

        cell_quota::register(
            cell,
            5,
            Limits {
                memory: PAGE_SIZE,
                guest: 0,
            },
        );
        let frame = super::OwnedFrame::allocate(Charge::new(cell, 5)).expect("fits");
        assert_eq!((used(), cell_quota::in_use(cell)), (Some(1), PAGE_SIZE));
        assert!(super::OwnedFrame::allocate(Charge::new(cell, 5)).is_none());
        assert_eq!(used(), Some(1));
        drop(frame);
        assert_eq!((used(), cell_quota::in_use(cell)), (Some(0), 0));

        cell_quota::deregister(cell);
        super::replace(previous);
    }

    /// Phase 16: past 1/64 of the managed frames in quarantine every data
    /// allocation is refused (and counted); page-table frames are exempt.
    #[test]
    fn the_quarantine_cap_refuses_data_frames_but_never_table_frames() {
        let set = |pages| super::quarantine_by_thread::PAGES.with(|count| count.set(pages));
        let refused = || crate::memory::frame_run::PENDING.counters().refused();
        // 128 frames: 2 quarantined pages are exactly 1/64 (allowed), 3 exceed it.
        let mut allocator = super::allocator_for_tests(&[(0x10_0000, 128)]);

        set(2);
        assert!(allocator.allocate_frame().is_some());
        assert!(allocator.allocate_contiguous(2).is_some());
        assert!(allocator
            .allocate_contiguous_aligned(2, 2 * PAGE_SIZE)
            .is_some());

        set(3);
        let used = allocator.used_frames();
        let before = refused();
        assert_eq!(allocator.allocate_frame(), None);
        assert_eq!(allocator.allocate_contiguous(1), None);
        assert_eq!(allocator.allocate_contiguous(2), None);
        assert_eq!(
            allocator.allocate_contiguous_aligned(2, 2 * PAGE_SIZE),
            None
        );
        assert!(refused() >= before + 4, "every refusal is counted");
        assert_eq!(allocator.used_frames(), used, "a refusal takes nothing");
        assert!(allocator.allocate_frame_uncapped().is_some());

        set(0);
        assert!(allocator.allocate_frame().is_some());
    }

    fn map_entry(base: usize, length: usize, ty: MemoryType) -> MemoryMapEntry {
        MemoryMapEntry { base, length, ty }
    }

    #[test]
    fn ram_at_or_above_the_cell_slots_is_left_unmanaged_and_counted() {
        let ceiling = crate::loader::va_alloc::IDENTITY_RAM_END;
        assert_eq!(ceiling, crate::loader::va_alloc::CELL_VA_START);
        // 2 GiB low, then a run straddling the ceiling by 16 MiB on each side,
        // then 1 GiB wholly above it.
        let entries = [
            map_entry(0x1000, 0x8000_0000 - 0x1000, MemoryType::Usable),
            map_entry(ceiling - 0x0100_0000, 0x0200_0000, MemoryType::Usable),
            map_entry(ceiling + 0x1_0000_0000, 0x4000_0000, MemoryType::Usable),
            map_entry(ceiling + 0x2_0000_0000, 0x1000, MemoryType::MMIO),
        ];
        let plan = plan_managed_ranges(&entries);
        let ranges: alloc::vec::Vec<_> = plan.ranges().collect();
        assert_eq!(ranges.len(), 2);
        let last = ranges[1];
        assert_eq!(last.start, ceiling - 0x0100_0000);
        assert_eq!(last.start + last.frames * PAGE_SIZE, ceiling);
        assert!(ranges
            .iter()
            .all(|range| range.start + range.frames * PAGE_SIZE <= ceiling));
        assert_eq!(plan.unmanaged_above(), 0x0100_0000 + 0x4000_0000);
        // Nothing above the ceiling: nothing unmanaged.
        assert_eq!(plan_managed_ranges(&entries[..1]).unmanaged_above(), 0);
        // An explicit lower ceiling cuts inside a run and drops the rest.
        let low = plan_managed_ranges_below(&entries, 0x4000_0000);
        assert_eq!(low.total_frames(), (0x4000_0000 - 0x1000) / PAGE_SIZE);
        assert_eq!(
            low.unmanaged_above(),
            0x4000_0000 + 0x0200_0000 + 0x4000_0000
        );
    }

    #[test]
    fn every_usable_range_is_managed() {
        // 256 MiB usable, a 16 MiB reserved hole, 128 MiB usable.
        let entries = [
            map_entry(0x8000_0000, 0x1000_0000, MemoryType::Usable),
            map_entry(0x9000_0000, 0x0100_0000, MemoryType::Reserved),
            map_entry(0xA000_0000, 0x0800_0000, MemoryType::Usable),
        ];
        let plan = plan_managed_ranges(&entries);
        assert_eq!(plan.range_count(), 2, "both usable ranges must be managed");
        assert_eq!(
            plan.total_frames(),
            (0x1000_0000 + 0x0800_0000) / PAGE_SIZE,
            "every usable frame contributes exactly once"
        );
        let ranges: alloc::vec::Vec<_> = plan.ranges().collect();
        assert_eq!(ranges[0].start, 0x8000_0000);
        assert_eq!(ranges[0].frames, 0x1000_0000 / PAGE_SIZE);
        assert_eq!(ranges[1].start, 0xA000_0000);
        assert_eq!(ranges[1].frames, 0x0800_0000 / PAGE_SIZE);
        // No managed frame may sit inside the reserved hole.
        for range in &ranges {
            assert!(
                range.start + range.frames * PAGE_SIZE <= 0x9000_0000 || range.start >= 0x9100_0000,
                "range {range:?} overlaps the reserved hole"
            );
        }
    }

    #[test]
    fn adjacent_usable_ranges_are_merged() {
        let entries = [
            map_entry(0x8000_0000, 0x0100_0000, MemoryType::Usable),
            map_entry(0x8100_0000, 0x0100_0000, MemoryType::Usable),
        ];
        let plan = plan_managed_ranges(&entries);
        assert_eq!(
            plan.range_count(),
            1,
            "adjacent usable entries are one range"
        );
        assert_eq!(plan.total_frames(), 0x0200_0000 / PAGE_SIZE);
    }
    #[test]
    fn frame_zero_is_never_managed() {
        let entries = [map_entry(0, 0x10_0000, MemoryType::Usable)];
        let plan = plan_managed_ranges(&entries);
        assert_eq!(plan.range_count(), 1);
        let ranges: alloc::vec::Vec<_> = plan.ranges().collect();
        assert_eq!(
            ranges[0].start, PAGE_SIZE,
            "physical address 0 must be excluded"
        );
        assert_eq!(ranges[0].frames, (0x10_0000 - PAGE_SIZE) / PAGE_SIZE);
    }

    #[test]
    #[should_panic(expected = "overlapping usable memory map entries")]
    fn overlapping_usable_ranges_halt() {
        let entries = [
            map_entry(0x8000_0000, 0x0100_0000, MemoryType::Usable),
            map_entry(0x8080_0000, 0x0100_0000, MemoryType::Usable),
        ];
        let _ = plan_managed_ranges(&entries);
    }

    /// A map of `ranges` disjoint usable one-page ranges, each followed by a
    /// one-page hole, so adjacency merging cannot reduce the count.
    fn disjoint_usable_map(ranges: usize) -> alloc::vec::Vec<MemoryMapEntry> {
        (0..ranges)
            .map(|i| map_entry(0x10_0000 + i * 2 * PAGE_SIZE, PAGE_SIZE, MemoryType::Usable))
            .collect()
    }

    /// Firmware maps carry many disjoint usable ranges (OVMF: more than eight);
    /// every range a converted boot map can hold must be managed.
    #[test]
    fn a_full_memory_map_of_disjoint_ranges_is_managed() {
        let full = crate::boot::MAX_MEMORY_MAP_ENTRIES;
        let plan = plan_managed_ranges(&disjoint_usable_map(full));
        assert_eq!(
            plan.range_count(),
            full,
            "every disjoint range is its own range"
        );
        assert_eq!(plan.total_frames(), full);
    }

    #[test]
    #[should_panic(expected = "more usable ranges than the allocator tracks (65 > 64)")]
    fn too_many_ranges_halt() {
        let _ = plan_managed_ranges(&disjoint_usable_map(super::MAX_MANAGED_RANGES + 1));
    }

    fn multi_range_allocator(ranges: &[(usize, usize)]) -> FrameAllocator {
        super::allocator_for_tests(ranges)
    }

    #[test]
    fn allocations_stay_inside_their_range() {
        // 8 frames at 0x1000 (0x1000..0x9000), a hole, 8 frames at 0x20000.
        let mut allocator = multi_range_allocator(&[(0x1000, 8), (0x20000, 8)]);
        assert_eq!(allocator.total_frames(), 16);
        assert!(allocator.manages(0x1000) && allocator.manages(0x20000));
        assert!(
            !allocator.manages(0x10000),
            "a hole is not managed; ranges={:?}",
            allocator.managed_ranges().collect::<alloc::vec::Vec<_>>()
        );

        let mut allocated = alloc::vec::Vec::new();
        while let Some(frame) = allocator.allocate_frame() {
            assert!(
                !(0x9000..0x20000).contains(&frame),
                "allocated a frame inside the hole: {frame:#x}"
            );
            allocated.push(frame);
        }
        assert_eq!(allocated.len(), 16, "every managed frame is allocatable");

        // Free everything, then ask for a run longer than any single range holds:
        // 16 frames are free, but they are 8 + 8 across a hole, so the request must
        // fail rather than span it.
        for frame in &allocated {
            allocator.deallocate_frame(*frame);
        }
        assert_eq!(allocator.free_frames(), 16);
        assert_eq!(allocator.allocate_contiguous(16), None);
        assert_eq!(allocator.allocate_contiguous(8), Some(0x1000));
        assert_eq!(allocator.allocate_contiguous(8), Some(0x20000));
    }

    /// The guest-RAM search takes the first run that fits.
    #[test]
    fn a_page_aligned_search_takes_the_first_fitting_run() {
        let mut allocator = allocator(32);
        // Used: [0, 8) and [16, 20); free: [8, 16) and [20, 32).
        allocator.mark_range_used(0, 8);
        allocator.mark_range_used(16, 4);
        let run = |n| allocator.find_free_run_aligned(n, PAGE_SIZE);
        assert_eq!(run(8), Some(8), "first run that fits");
        assert_eq!(run(9), Some(20), "skips the 8-frame run");
        assert_eq!(run(12), Some(20));
        assert_eq!(run(13), None, "12 + 8 frames, no single run");
        assert_eq!(run(0), None, "a zero-frame request is not a run");
        assert_eq!(allocator.largest_free_run(), 12);
    }

    /// Two ranges with a hole between them: their frames are consecutive in
    /// index space, but a run across the boundary is not contiguous memory, so
    /// it must not be handed out as guest RAM (a 2048 MiB guest on a 3 GiB
    /// machine got one and died in early boot).
    #[test]
    fn a_guest_run_never_spans_the_hole_between_ranges() {
        let mut allocator = multi_range_allocator(&[(0x1000, 8), (0x20000, 8)]);
        assert_eq!(allocator.find_free_run_aligned(16, PAGE_SIZE), None);
        assert_eq!(allocator.find_free_run_aligned(9, PAGE_SIZE), None);
        // Index 4 starts a free tail of 4 frames in the first range and 8 more
        // in the second: 12 by index, 4 by memory.
        allocator.mark_range_used(0, 4);
        assert_eq!(allocator.find_free_run_aligned(5, PAGE_SIZE), Some(8));
        assert_eq!(allocator.frame_addr(8), 0x20000);
    }

    /// A carve marks the run it returns, so the next search cannot hand the same
    /// frames out twice.
    #[test]
    fn a_carved_run_is_marked_used_in_the_same_pass() {
        let mut allocator = allocator(32);
        let start = allocator
            .find_free_run_aligned(20, PAGE_SIZE)
            .expect("20 frames fit");
        assert_eq!(start, 0);
        allocator.mark_range_used(start, 20);
        assert_eq!(allocator.used_frames(), 20);
        assert_eq!(
            allocator.find_free_run_aligned(20, PAGE_SIZE),
            None,
            "the run is gone"
        );
        assert_eq!(allocator.largest_free_run(), 12);
    }

    /// Guest RAM wants a run that starts on a 2 MiB boundary (here: 8 frames).
    /// The fixture's first frame is at 4 KiB, so index `i` is frame `i + 1`.
    #[test]
    fn an_aligned_run_starts_on_the_boundary_and_skips_used_frames() {
        const ALIGN: usize = 8 * PAGE_SIZE;
        let mut allocator = allocator(64);
        allocator.mark_range_used(10, 1);
        // Index 7 (frame 8) is the first boundary; its run would cover index 10.
        assert_eq!(allocator.find_free_run_aligned(3, ALIGN), Some(7));
        assert_eq!(allocator.find_free_run_aligned(4, ALIGN), Some(15));
        // From index 15 to the end are 49 frames, and no later boundary has more.
        assert_eq!(allocator.find_free_run_aligned(49, ALIGN), Some(15));
        assert_eq!(allocator.find_free_run_aligned(50, ALIGN), None);
        assert_eq!(
            allocator.frame_addr(15) % ALIGN,
            0,
            "the run's address is aligned"
        );
        assert_eq!(allocator.find_free_run_aligned(4, 3 * PAGE_SIZE), None);
    }

    fn allocator(total_frames: usize) -> FrameAllocator {
        let mut ranges: [Option<super::ManagedRange>; super::MAX_MANAGED_RANGES] =
            [None; super::MAX_MANAGED_RANGES];
        ranges[0] = Some(super::ManagedRange {
            start: PAGE_SIZE,
            frames: total_frames,
            index_base: 0,
        });
        FrameAllocator {
            ranges,
            range_count: 1,
            total_frames,
            used_frames: 0,
            bitmap: super::BitmapStorage::Owned(
                alloc::vec![0u64; total_frames.div_ceil(64)].into_boxed_slice(),
            ),
            bitmap_range: 0,
            last_alloc_index: 0,
            #[cfg(feature = "p01-trace")]
            p01_epoch: 1,
            #[cfg(feature = "p01-trace")]
            p01_peak: 0,
        }
    }

    #[test]
    fn accounting_changes_only_on_bitmap_transitions() {
        let mut allocator = allocator(32);
        let frame = allocator.allocate_frame().expect("single frame");
        assert_eq!(allocator.used_frames(), 1);
        assert_eq!(allocator.free_frames(), 31);

        allocator.mark_range_used(0, 1);
        assert_eq!(
            allocator.used_frames(),
            1,
            "repeated mark must be idempotent"
        );

        allocator.deallocate_frame(frame);
        allocator.deallocate_frame(frame);
        assert_eq!(allocator.used_frames(), 0, "double free must not underflow");
        assert_eq!(allocator.total_frames(), allocator.free_frames());
    }

    #[test]
    fn boot_heap_run_must_be_contiguous_or_refused() {
        // Plenty of free frames, but no 1,024-frame run: every 100th frame is taken.
        let mut allocator = allocator(2_048);
        for hole in (50..2_048).step_by(100) {
            allocator.mark_range_used(hole, 1);
        }
        let before = allocator.used_frames();
        let marked_before: alloc::vec::Vec<bool> = (0..2_048)
            .map(|i| allocator.is_frame_allocated(i))
            .collect();
        assert!(
            allocator.free_frames() > 1_024,
            "fixture must leave more free frames than the run needs"
        );

        match reserve_contiguous_run(&mut allocator, 1_024) {
            Some(start) => {
                // The heap is initialized over `start .. start + 1024`, so the
                // reservation must allocate exactly that span — not the same number
                // of scattered frames that happen to cover it.
                let first = ((start - PAGE_SIZE) / PAGE_SIZE) as usize;
                for index in 0..2_048 {
                    let newly_marked = !marked_before[index] && allocator.is_frame_allocated(index);
                    let inside_run = index >= first && index < first + 1_024;
                    assert_eq!(
                        newly_marked,
                        inside_run,
                        "frame {index}: the reservation must allocate exactly the run \
                         {first}..{}",
                        first + 1_024
                    );
                }
                assert_eq!(allocator.used_frames(), before + 1_024);
            }
            None => assert_eq!(
                allocator.used_frames(),
                before,
                "a refused heap reservation must not mark any frame"
            ),
        }
    }

    #[test]
    fn boot_heap_run_after_a_gap_is_reserved_whole() {
        let mut allocator = allocator(2_048);
        allocator.mark_range_used(0, 64);
        let before = allocator.used_frames();
        let expected_first = (0..2_048)
            .find(|index| !allocator.is_frame_allocated(*index))
            .expect("the map has free frames");

        let start =
            reserve_contiguous_run(&mut allocator, 1_024).expect("a 1,024-frame run exists");
        assert_eq!(
            start,
            PAGE_SIZE * (expected_first + 1),
            "the run starts at the first free frame"
        );
        for index in expected_first..expected_first + 1_024 {
            assert!(
                allocator.is_frame_allocated(index),
                "frame {index} must be part of the reserved run"
            );
        }
        assert_eq!(allocator.used_frames(), before + 1_024);
    }

    #[test]
    fn contiguous_and_aligned_allocations_account_exactly() {
        let mut allocator = allocator(32);
        allocator.allocate_contiguous(3).expect("contiguous run");
        assert_eq!(allocator.used_frames(), 3);
        let aligned = allocator
            .allocate_contiguous_aligned(2, PAGE_SIZE * 2)
            .expect("aligned run");
        assert_eq!(aligned % (PAGE_SIZE * 2), 0);
        assert_eq!(allocator.used_frames(), 5);
        assert_eq!(
            allocator.total_frames(),
            allocator.used_frames() + allocator.free_frames()
        );
    }

    /// `HEAP_FRAMES` of the 64-bit boot path in `main.rs` (32 MiB).
    const BOOT_HEAP_FRAMES: usize = 8_192;

    /// The allocator `FrameAllocator::new_from_map` builds over `ranges` (the
    /// `[boot]   range start..end` lines a boot logs), with an owned bitmap, and
    /// the physical address of the bitmap's first frame.
    fn boot_allocator(ranges: &[(usize, usize)]) -> (FrameAllocator, usize) {
        let entries: alloc::vec::Vec<_> = ranges
            .iter()
            .map(|&(start, end)| map_entry(start, end - start, MemoryType::Usable))
            .collect();
        let plan = plan_managed_ranges(&entries);
        let bitmap_start = plan.bitmap_start();
        let words = plan.total_frames().div_ceil(64);
        let bitmap = super::BitmapStorage::Owned(alloc::vec![0u64; words].into_boxed_slice());
        (FrameAllocator::from_plan(plan, bitmap), bitmap_start)
    }

    /// The boot path up to `register(bitmap start, heap start)` in `main.rs`:
    /// `early` single frames (paging and firmware mappings), then the heap.
    /// Returns the allocator, the bitmap's first frame and the heap.
    fn boot_heap(ranges: &[(usize, usize)], early: usize) -> (FrameAllocator, usize, usize) {
        let (mut allocator, bitmap_start) = boot_allocator(ranges);
        for _ in 0..early {
            allocator.allocate_frame().expect("early boot frame");
        }
        let heap = reserve_boot_heap(&mut allocator, BOOT_HEAP_FRAMES)
            .expect("the bitmap's range holds the boot heap");
        (allocator, bitmap_start, heap)
    }

    /// The span `main.rs` registers as shared kernel memory, `[bitmap start, heap
    /// start)`, is a non-empty run of one managed range in which every frame is
    /// already owned (the bitmap or an early boot frame), and the heap follows it.
    fn assert_registered_span_is_owned(
        allocator: &FrameAllocator,
        bitmap_start: usize,
        heap: usize,
    ) {
        assert!(
            bitmap_start < heap,
            "registration needs start < end: bitmap {bitmap_start:#x}, heap {heap:#x}"
        );
        let range = allocator
            .managed_ranges()
            .find(|range| {
                bitmap_start >= range.start && bitmap_start < range.start + range.frames * PAGE_SIZE
            })
            .expect("the bitmap is managed");
        assert_eq!(bitmap_start, range.start, "the bitmap opens its range");
        assert!(
            heap + BOOT_HEAP_FRAMES * PAGE_SIZE <= range.start + range.frames * PAGE_SIZE,
            "the heap {heap:#x} lies in the bitmap's range {range:?}"
        );
        for frame in (bitmap_start..heap + BOOT_HEAP_FRAMES * PAGE_SIZE).step_by(PAGE_SIZE) {
            let index = allocator.index_of_addr(frame).expect("managed frame");
            assert!(
                allocator.is_frame_allocated(index),
                "frame {frame:#x} inside the registered span or heap is free"
            );
        }
    }

    /// VMware Workstation 25, EFI, 1024 MiB (S3 V0 run `efi-m1024`), and QEMU q35
    /// + OVMF, 4096 MiB: the largest range (the bitmap's) is not the first range
    /// with 32 MiB free, so a first fit put the heap below the bitmap and
    /// `register(bitmap start, heap start)` panicked at boot.
    #[test]
    fn boot_heap_follows_the_bitmap_on_uefi_maps() {
        let vmware_efi_1024 = [
            (0x3000, 0xa0000),
            (0x10_0000, 0xdf2_d000),
            (0xdf7_3000, 0xfb6_f000),
            (0xfbf_f000, 0x39ab_4000),
            (0x3c00_0000, 0x3fff_6000),
            (0x3fff_9000, 0x4000_0000),
        ];
        let ovmf_4096 = [
            (0x0, 0xa0000),
            (0x10_0000, 0x80_0000),
            (0x80_8000, 0x80_b000),
            (0x80_c000, 0x81_0000),
            (0x90_0000, 0x7800_5000),
            (0x7a3f_7000, 0x7bf4_e000),
            (0x7bfe_4000, 0x7e4d_8000),
            (0x7e4d_e000, 0x7e4e_4000),
            (0x7e55_b000, 0x7ea4_d000),
            (0x7ea4_f000, 0x7ea5_5000),
            (0x7ea5_8000, 0x7f00_e000),
            (0x7f0c_f000, 0x7f8e_d000),
            (0x7fbf_f000, 0x7fe0_0000),
            (0x7fe7_7000, 0x7fef_4000),
            (0x1_0000_0000, 0x1_8000_0000),
        ];
        for map in [&vmware_efi_1024[..], &ovmf_4096[..]] {
            let (mut first_fit, bitmap_start) = boot_allocator(map);
            let below = reserve_contiguous_run(&mut first_fit, BOOT_HEAP_FRAMES)
                .expect("a first fit finds a run");
            assert!(
                below < bitmap_start,
                "fixture: a first fit lands below the bitmap ({below:#x} < {bitmap_start:#x})"
            );

            let (allocator, bitmap_start, heap) = boot_heap(map, 64);
            assert_registered_span_is_owned(&allocator, bitmap_start, heap);
        }
    }

    /// Where the heap already followed the bitmap — a BIOS map (VMware, 4096 MiB)
    /// and a single-range map whose early frames follow the bitmap (the RV64 /
    /// AArch64 shape) — the boot heap lands exactly where the first fit put it.
    #[test]
    fn boot_heap_is_unchanged_where_it_already_followed_the_bitmap() {
        let vmware_bios_4096 = [
            (0x5_a000, 0x9_f000),
            (0x10_0000, 0xbda5_5000),
            (0x1_0000_0000, 0x1_4000_0000),
        ];
        let single_range = [(0x8020_0000, 0x8800_0000)];
        for map in [&vmware_bios_4096[..], &single_range[..]] {
            let (mut first_fit, _) = boot_allocator(map);
            for _ in 0..64 {
                first_fit.allocate_frame().expect("early boot frame");
            }
            let expected = reserve_contiguous_run(&mut first_fit, BOOT_HEAP_FRAMES)
                .expect("a first fit finds a run");

            let (allocator, bitmap_start, heap) = boot_heap(map, 64);
            assert_eq!(heap, expected, "the heap does not move");
            assert_registered_span_is_owned(&allocator, bitmap_start, heap);
        }
        // Single range: the early frames sit between the bitmap and the heap and
        // are part of the registered span.
        let (_, bitmap_start, heap) = boot_heap(&single_range, 64);
        let frames = (0x8800_0000 - 0x8020_0000) / PAGE_SIZE;
        let bitmap_pages = (frames.div_ceil(64) * 8).div_ceil(PAGE_SIZE);
        assert_eq!(heap, bitmap_start + (bitmap_pages + 64) * PAGE_SIZE);
    }

    /// No run in the bitmap's range refuses the heap, marks nothing, and does not
    /// fall back to another range — that would break the registered span.
    #[test]
    fn boot_heap_refuses_rather_than_leave_the_bitmap_range() {
        // Two equal ranges: the bitmap takes the second (the last of equal maxima),
        // so only the first still has a free 32 MiB run.
        let size = BOOT_HEAP_FRAMES * PAGE_SIZE;
        let map = [
            (0x10_0000, 0x10_0000 + size),
            (0x1_0000_0000, 0x1_0000_0000 + size),
        ];
        let (mut allocator, bitmap_start) = boot_allocator(&map);
        assert_eq!(
            bitmap_start, 0x1_0000_0000,
            "fixture: the bitmap is in the second range"
        );
        let used = allocator.used_frames();
        assert_eq!(reserve_boot_heap(&mut allocator, BOOT_HEAP_FRAMES), None);
        assert_eq!(
            allocator.used_frames(),
            used,
            "a refused heap marks no frame"
        );
        assert_eq!(
            reserve_contiguous_run(&mut allocator, BOOT_HEAP_FRAMES),
            Some(0x10_0000),
            "fixture: another range had room"
        );
    }
}

/// Allocate N contiguous physical frames for guest VM RAM.
///
/// One lock hold, one linear pass. The scan used to release `FRAME_ALLOCATOR`
/// every `PROBE_CHUNK` frames so a long search could not trip the RT watchdog,
/// but that let any other allocation split the run being counted: the same image
/// created its 128 MiB guest on one board boot and failed on the next with no
/// layout change (`[hv] create_vm: no contiguous guest run`). Counting a run
/// over the bitmap is a read per frame — tens of kilobytes for a 512 MiB
/// request — so the whole search belongs inside the lock, and the frames are
/// marked in the same hold. There is no window left in which a candidate run can
/// be taken by someone else.
///
/// The run starts 2 MiB-aligned when the allocator has such a run: the nested
/// page table can then map it with 2 MiB leaves. Otherwise any run inside one
/// managed range will do.
pub fn allocate_guest_ram(n_pages: usize) -> Option<PhysAddr> {
    const LARGE_LEAF: usize = 2 * 1024 * 1024;
    with(|allocator| {
        if allocator.quarantine_refuses() {
            return None;
        }
        let start = allocator
            .find_free_run_aligned(n_pages, LARGE_LEAF)
            .or_else(|| allocator.find_free_run_aligned(n_pages, PAGE_SIZE))?;
        allocator.mark_range_used(start, n_pages);
        Some(allocator.frame_addr(start))
    })
    .flatten()
}

/// Longest free run the allocator has right now, in frames.
///
/// Logged when a guest carve fails, so the next report says whether the map is
/// fragmented or simply too small.
pub fn largest_free_run() -> usize {
    with(|allocator| allocator.largest_free_run()).unwrap_or(0)
}

/// Test-hooks: frames whose allocation bit differs from `before`, capped.
///
/// `before` must come from [`FrameAllocator::used_bitmap`]. Used together with
/// [`free_frames`] it separates "the count moved" from "these frames moved":
/// the first is what a ledger test asserts, the second is what names the defect.
#[cfg(feature = "test-hooks")]
pub fn changed_frames(before: &[u64]) -> alloc::vec::Vec<(PhysAddr, bool)> {
    /// A ledger delta is expected to be one or two frames. Anything larger is a
    /// different failure and does not need a per-frame listing to be read.
    const CAP: usize = 32;
    let mut changed = alloc::vec::Vec::new();
    let guard = FRAME_ALLOCATOR.lock();
    let Some(allocator) = guard.as_ref() else {
        return changed;
    };
    for (index, (was, now)) in before
        .iter()
        .zip(allocator.bitmap.slice().iter())
        .enumerate()
    {
        let mut differences = was ^ now;
        while differences != 0 {
            let bit = differences.trailing_zeros() as usize;
            differences &= differences - 1;
            let frame = index * 64 + bit;
            if frame >= allocator.total_frames {
                continue;
            }
            changed.push((allocator.frame_addr(frame), (now >> bit) & 1 != 0));
            if changed.len() == CAP {
                return changed;
            }
        }
    }
    changed
}
