//! Physical frame allocator for ViOS kernel.
//!
//! Manages physical memory frames (4KB pages) using a Bitmap Allocator.
//! This allows for O(1) allocation and deallocation (amortized) and frame reuse.

use crate::boot::MemoryMapEntry;
use crate::*;

// Define PAGE_SIZE to avoid circular dependency with paging.rs
const PAGE_SIZE: usize = 4096;

/// Bitmap Frame Allocator
pub struct FrameAllocator {
    /// Start of usable memory managed by this allocator
    memory_start: PhysAddr,
    /// End of usable memory
    memory_end: PhysAddr,
    /// Total frames managed
    total_frames: usize,
    /// Bitmap storage (borrowed from reserved memory)
    bitmap: &'static mut [u64],
    /// Index of the last allocated frame (for next-fit search)
    last_alloc_index: usize,
}

impl FrameAllocator {
    /// Initialize allocator from memory map
    ///
    /// This function finds the largest usable memory region, reserves space for the bitmap
    /// at the beginning of that region, and initializes the allocator.
    pub fn new_from_map(entries: &[MemoryMapEntry]) -> Self {
        let mut best_start = 0;
        let mut best_end = 0;
        let mut max_len = 0;

        // 1. Find largest usable region
        for entry in entries {
            if entry.ty == crate::boot::MemoryType::Usable {
                if entry.length > max_len {
                    max_len = entry.length;
                    best_start = entry.base;
                    best_end = entry.base + entry.length;
                }
            }
        }

        // Align start to 4KB
        let aligned_start = (best_start + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let aligned_end = best_end & !(PAGE_SIZE - 1);
        let available_size = aligned_end - aligned_start;
        let total_frames = available_size / PAGE_SIZE;

        // 2. Calculate bitmap size
        // We need 1 bit per frame.
        // 1 u64 = 64 bits = 64 frames.
        // Bitmap size in u64s = (total_frames + 63) / 64
        let bitmap_u64_count = (total_frames + 63) / 64;
        let bitmap_size_bytes = bitmap_u64_count * 8;

        // 3. Place bitmap at the beginning of the region
        // We need to reserve enough *pages* for the bitmap
        let bitmap_pages = (bitmap_size_bytes + PAGE_SIZE - 1) / PAGE_SIZE;
        let bitmap_phys_addr = aligned_start;

        // 4. Create the bitmap slice
        // SAFETY: We own this memory region and we are single-threaded (or locked) at init.
        let bitmap = unsafe {
            core::slice::from_raw_parts_mut(bitmap_phys_addr as *mut u64, bitmap_u64_count)
        };

        // 5. Initialize bitmap
        // Initially, we mark ALL frames as FREE (0).
        // Then we mark the frames used by the bitmap itself as USED (1).
        for i in 0..bitmap_u64_count {
            bitmap[i] = 0;
        }

        // 6. Adjust allocator start to after the bitmap
        // But wait, the bitmap index 0 corresponds to `aligned_start`.
        // So we just need to mark the first `bitmap_pages` frames as used.

        let mut allocator = Self {
            memory_start: aligned_start,
            memory_end: aligned_end,
            total_frames,
            bitmap,
            last_alloc_index: 0,
        };

        // Mark bitmap pages as used
        for i in 0..bitmap_pages {
            allocator.mark_used(i);
        }

        allocator
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

    /// Deallocate a physical frame
    pub fn deallocate_frame(&mut self, frame: PhysAddr) {
        if let Some(idx) = self.addr_to_frame_index(frame) {
            self.mark_free(idx);
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

            let block = self.bitmap[u64_idx];

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

    fn mark_used(&mut self, idx: usize) {
        let u64_idx = idx / 64;
        let bit_offset = idx % 64;
        self.bitmap[u64_idx] |= 1u64 << bit_offset;
    }

    fn mark_free(&mut self, idx: usize) {
        let u64_idx = idx / 64;
        let bit_offset = idx % 64;
        self.bitmap[u64_idx] &= !(1u64 << bit_offset);
    }

    fn frame_index_to_addr(&self, idx: usize) -> PhysAddr {
        self.memory_start + (idx * PAGE_SIZE)
    }

    fn addr_to_frame_index(&self, addr: PhysAddr) -> Option<usize> {
        if addr < self.memory_start || addr >= self.memory_end {
            return None;
        }
        Some((addr - self.memory_start) / PAGE_SIZE)
    }

    /// Get total available memory in bytes
    pub fn total_memory(&self) -> usize {
        self.total_frames * PAGE_SIZE
    }

    /// Get used memory in bytes (Approximate)
    pub fn used_memory(&self) -> usize {
        // Counting bits is expensive, just return total - free is better but we don't track free count.
        // For now, let's just return 0 or implement counting later.
        // This is mainly for stats.
        0
    }
}

/// Global frame allocator
pub static FRAME_ALLOCATOR: crate::sync::Spinlock<Option<FrameAllocator>> =
    crate::sync::Spinlock::new(None);
