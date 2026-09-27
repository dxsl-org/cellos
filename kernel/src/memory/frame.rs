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
    /// Index of the last allocated frame (for next-fit search)
    last_alloc_index: usize,
}

impl FrameAllocator {
    /// Initialize allocator from memory map
    ///
    /// Every usable range the map describes is managed (see [`plan_managed_ranges`]);
    /// the bitmap is placed at the start of the largest range and its own frames are
    /// marked used.
    pub fn new_from_map(entries: &[MemoryMapEntry]) -> Self {
        let plan = plan_managed_ranges(entries);
        let bitmap_start = plan.bitmap_start();
        let bitmap_u64_count = plan.total_frames().div_ceil(64);

        // SAFETY: the plan guarantees the largest range can hold the bitmap, and the
        // allocator is constructed once during single-threaded boot.
        let bitmap = unsafe {
            core::slice::from_raw_parts_mut(
                phys_to_virt(bitmap_start) as *mut u64,
                bitmap_u64_count,
            )
        };
        for slot in bitmap.iter_mut() {
            *slot = 0;
        }

        let mut allocator = Self {
            ranges: plan.ranges,
            range_count: plan.range_count(),
            total_frames: plan.total_frames(),
            used_frames: 0,
            bitmap: BitmapStorage::Borrowed(bitmap),
            last_alloc_index: 0,
        };

        // The bitmap lives inside managed memory, so its own frames are reserved.
        let bitmap_pages = (bitmap_u64_count * 8).div_ceil(PAGE_SIZE);
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
            .find(|range| {
                addr >= range.start && addr < range.start + range.frames * PAGE_SIZE
            })
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

    /// Allocate a physical frame
    pub fn allocate_frame(&mut self) -> Option<PhysAddr> {
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

    // --- Helper bits ---

    fn find_free(&self, start_idx: usize, end_idx: usize) -> Option<usize> {
        let mut bit_idx = start_idx;
        while bit_idx < end_idx {
            let u64_idx = bit_idx / 64;
            let bit_offset = bit_idx % 64;

            let block = self.bitmap.slice()[u64_idx];

            // Optimization: Skip full blocks
            if block == !0 {
                // All 1s
                bit_idx = (u64_idx + 1) * 64;
                continue;
            }

            // Check if specific bit is 0
            if (block & (1u64 << bit_offset)) == 0 {
                return Some(bit_idx);
            }
            bit_idx += 1;
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
        self.bitmap.slice_mut()[u64_idx] |= mask;
        self.used_frames += 1;
        true
    }

    fn mark_free(&mut self, idx: usize) -> bool {
        let u64_idx = idx / 64;
        let bit_offset = idx % 64;
        let mask = 1u64 << bit_offset;
        if self.bitmap.slice()[u64_idx] & mask == 0 {
            return false;
        }
        self.bitmap.slice_mut()[u64_idx] &= !mask;
        self.used_frames -= 1;
        true
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
        // No allocation here: this runs during boot (the heap reservation) before the
        // global allocator exists.
        for range_index in 0..self.range_count {
            let Some(range) = self.ranges[range_index] else {
                continue;
            };
            if range.frames < n {
                continue;
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

/// Global frame allocator
pub static FRAME_ALLOCATOR: crate::sync::Spinlock<Option<FrameAllocator>> =
    crate::sync::Spinlock::new(None);

/// A non-copyable allocation lease returned to the bitmap only on drop.
pub struct OwnedFrame(PhysAddr);

impl OwnedFrame {
    pub fn allocate() -> Option<Self> {
        FRAME_ALLOCATOR.lock().as_mut()?.allocate_frame().map(Self)
    }

    pub const fn physical_address(&self) -> PhysAddr {
        self.0
    }
}

impl Drop for OwnedFrame {
    fn drop(&mut self) {
        if let Some(frames) = FRAME_ALLOCATOR.lock().as_mut() {
            frames.deallocate_frame(self.0);
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

/// Largest number of disjoint usable ranges the allocator tracks.
///
/// Firmware maps in the supported targets describe at most a handful (QEMU virt
/// and the supported boards report one; a Limine map with a reserved hole reports
/// two). More than this halts rather than silently dropping memory.
pub const MAX_MANAGED_RANGES: usize = 8;

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

    /// Physical address of the range that hosts the bitmap.
    pub fn bitmap_start(&self) -> PhysAddr {
        self.ranges[self.bitmap_range]
            .expect("the bitmap range is always present")
            .start
    }
}

/// Normalize a memory map into the set of usable ranges the allocator manages.
///
/// Malformed authoritative input (an entry that overlaps another, a usable entry
/// smaller than a page, or more ranges than [`MAX_MANAGED_RANGES`]) **halts**: a
/// wrong guess here is memory corruption, not a degraded boot.
pub fn plan_managed_ranges(entries: &[MemoryMapEntry]) -> ManagedPlan {
    let mut ranges: [Option<ManagedRange>; MAX_MANAGED_RANGES] = [None; MAX_MANAGED_RANGES];
    let mut count = 0usize;
    let mut total_frames = 0usize;

    // This runs before the heap exists, so the plan is built without allocating:
    // repeatedly select the lowest usable run at or after `cursor`.
    let mut cursor: PhysAddr = 0;
    loop {
        let mut best: Option<(PhysAddr, PhysAddr)> = None;
        for entry in entries {
            if entry.ty != crate::boot::MemoryType::Usable || entry.length == 0 {
                continue;
            }
            let start = (entry.base + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
            let end = (entry.base + entry.length) & !(PAGE_SIZE - 1);
            if end <= start || start < cursor {
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
    for entry in entries {
        if entry.ty != crate::boot::MemoryType::Usable || entry.length == 0 {
            continue;
        }
        let start = (entry.base + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let end = (entry.base + entry.length) & !(PAGE_SIZE - 1);
        if end <= start {
            // Smaller than one page: it contributes no frame.
            continue;
        }
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
    }
}

#[cfg(test)]
mod tests {
    use super::{plan_managed_ranges, reserve_contiguous_run, FrameAllocator, PAGE_SIZE};
    use crate::boot::{MemoryMapEntry, MemoryType};

    fn map_entry(base: usize, length: usize, ty: MemoryType) -> MemoryMapEntry {
        MemoryMapEntry {
            base,
            length,
            ty,
        }
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
                range.start + range.frames * PAGE_SIZE <= 0x9000_0000
                    || range.start >= 0x9100_0000,
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
        assert_eq!(plan.range_count(), 1, "adjacent usable entries are one range");
        assert_eq!(plan.total_frames(), 0x0200_0000 / PAGE_SIZE);
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

    #[test]
    #[should_panic(expected = "more usable ranges than")]
    fn too_many_ranges_halt() {
        let entries: alloc::vec::Vec<_> = (0..9)
            .map(|i| map_entry(0x8000_0000 + i * 0x0200_0000, 0x0100_0000, MemoryType::Usable))
            .collect();
        let _ = plan_managed_ranges(&entries);
    }

    fn multi_range_allocator(ranges: &[(usize, usize)]) -> FrameAllocator {
        let mut slots: [Option<super::ManagedRange>; super::MAX_MANAGED_RANGES] =
            [None; super::MAX_MANAGED_RANGES];
        let mut total = 0usize;
        for (index, (start, frames)) in ranges.iter().enumerate() {
            slots[index] = Some(super::ManagedRange {
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
            bitmap: super::BitmapStorage::Owned(
                alloc::vec![0u64; total.div_ceil(64)].into_boxed_slice(),
            ),
            last_alloc_index: 0,
        }
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
            last_alloc_index: 0,
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
        let marked_before: alloc::vec::Vec<bool> =
            (0..2_048).map(|i| allocator.is_frame_allocated(i)).collect();
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
                        newly_marked, inside_run,
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

        let start = reserve_contiguous_run(&mut allocator, 1_024).expect("a 1,024-frame run exists");
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
}

/// Allocate N contiguous physical frames for guest VM RAM using a chunked scan.
///
/// Releases `FRAME_ALLOCATOR` every `PROBE_CHUNK` frames during the search phase
/// to keep lock-hold time bounded and prevent the RT watchdog from firing during
/// a 512 MiB (131 072-frame) contiguous search (Red-Team M2).
///
/// # TOCTOU
/// After locating a candidate run, the lock is re-acquired to re-verify and mark
/// all frames atomically.  Transparent on QEMU TCG (single CPU).  A production
/// SMP build would use a buddy allocator with a free-run index.
pub fn allocate_guest_ram(n_pages: usize) -> Option<PhysAddr> {
    const PROBE_CHUNK: usize = 256;

    let total = FRAME_ALLOCATOR.lock().as_ref()?.total_frames;
    let limit = total.saturating_sub(n_pages);
    let mut candidate = 0usize; // current candidate run start (frame index)
    let mut run_len = 0usize; // confirmed free frames from candidate onward

    while candidate <= limit {
        let probe_from = candidate + run_len;
        if probe_from >= total {
            break;
        }

        // Probe up to PROBE_CHUNK frames under a bounded lock hold.
        let (chunk_free, first_used) = {
            let g = FRAME_ALLOCATOR.lock();
            let a = g.as_ref()?;
            let probe_end = (probe_from + PROBE_CHUNK).min(total);
            let mut free_cnt = 0usize;
            let mut used_at = None;
            for idx in probe_from..probe_end {
                if a.is_frame_allocated(idx) {
                    used_at = Some(idx);
                    break;
                }
                free_cnt += 1;
            }
            (free_cnt, used_at)
        }; // lock dropped — other kernel tasks may run

        run_len += chunk_free;

        if run_len >= n_pages {
            // Candidate run is long enough — re-verify and allocate under lock.
            let result = {
                let mut g = FRAME_ALLOCATOR.lock();
                let a = g.as_mut()?;
                let all_free = (0..n_pages).all(|i| !a.is_frame_allocated(candidate + i));
                if all_free {
                    a.mark_range_used(candidate, n_pages);
                    Some(a.frame_addr(candidate))
                } else {
                    None // race: another CPU grabbed a frame; restart
                }
            };
            if let Some(pa) = result {
                return Some(pa);
            }
            candidate += 1;
            run_len = 0;
            continue;
        }

        if let Some(used_idx) = first_used {
            candidate = used_idx + 1;
            run_len = 0;
        }
        // If chunk was all free but run_len < n_pages: loop to probe next chunk.
    }
    None
}
