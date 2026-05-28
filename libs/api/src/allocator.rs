// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Memory allocation interfaces for ViOS.
//!
//! Provides arena allocators and custom allocation strategies
//! to reduce fragmentation and improve performance.

// Allow unsafe for GlobalAllocator trait (requires unsafe methods)
#![allow(unsafe_code)]

use crate::*;

/// Arena allocator for trait objects and temporary allocations.
///
/// # Purpose
/// Reduce allocation overhead and fragmentation by:
/// - Batch allocation (bump pointer)
/// - Batch deallocation (reset entire arena)
/// - Thread-local arenas (no contention)
///
/// # Performance Guarantees
/// - `alloc()`: O(1) - bump pointer increment + alignment
/// - `reset()`: O(1) - single pointer write
/// - `used_bytes()`: O(1) - field access
/// - `capacity()`: O(1) - constant
/// - `can_alloc()`: O(1) - arithmetic check
///
/// # Benchmarks (Target)
/// - Allocation: <50 cycles (vs ~500 for malloc)
/// - Batch deallocation: <5 cycles (vs O(n) for individual frees)
/// - Memory overhead: <1% (vs ~10% for general allocator)
///
/// # Use Cases
/// - Allocating multiple trait objects during request handling
/// - Temporary buffers for I/O operations
/// - Per-task scratch space
pub trait ViArenaAllocator: Send + Sync {
    /// Allocate memory with specified size and alignment.
    ///
    /// # Arguments
    /// * `size` - Size in bytes
    /// * `align` - Alignment requirement (power of 2)
    ///
    /// # Returns
    /// Pointer to allocated memory, or error if arena is full.
    ///
    /// # Safety
    /// Caller must ensure proper deallocation via `reset()`.
    ///
    /// # Performance
    /// O(1) - bump pointer increment
    fn alloc(&mut self, size: usize, align: usize) -> ViResult<*mut u8>;

    /// Allocate space for a trait object.
    ///
    /// # Returns
    /// Pointer suitable for trait object construction.
    ///
    /// # Note
    /// Allocates space for a fat pointer (2 * pointer size).
    ///
    /// # Performance
    /// O(1) - same as alloc()
    fn alloc_trait_object(&mut self) -> ViResult<*mut u8> {
        // Fat pointer = 2 * usize (data ptr + vtable ptr)
        let size = core::mem::size_of::<usize>() * 2;
        let align = core::mem::align_of::<usize>();
        self.alloc(size, align)
    }

    /// Reset arena, deallocating all allocations at once.
    ///
    /// # Performance
    /// O(1) - Just resets bump pointer.
    fn reset(&mut self);

    /// Get current memory usage.
    ///
    /// # Performance
    /// O(1) - field access
    fn used_bytes(&self) -> usize;

    /// Get total arena capacity.
    ///
    /// # Performance
    /// O(1) - constant
    fn capacity(&self) -> usize;

    /// Check if arena has space for allocation.
    ///
    /// # Performance
    /// O(1) - arithmetic check
    fn can_alloc(&self, size: usize, align: usize) -> bool {
        // Default implementation
        self.used_bytes() + size + align <= self.capacity()
    }
}

/// Global allocator interface for kernel heap.
pub trait ViGlobalAllocator: Send + Sync {
    /// Allocate memory from global heap.
    ///
    /// # Safety
    /// Must be paired with `dealloc()` call.
    unsafe fn alloc(&self, size: usize, align: usize) -> ViResult<*mut u8>;

    /// Deallocate memory.
    ///
    /// # Safety
    /// `ptr` must have been allocated by this allocator.
    unsafe fn dealloc(&self, ptr: *mut u8, size: usize, align: usize);

    /// Reallocate memory.
    ///
    /// # Safety
    /// `ptr` must have been allocated by this allocator.
    unsafe fn realloc(
        &self,
        ptr: *mut u8,
        old_size: usize,
        new_size: usize,
        align: usize,
    ) -> ViResult<*mut u8>;

    /// Get total allocated bytes.
    fn total_allocated(&self) -> usize;

    /// Get total free bytes.
    fn total_free(&self) -> usize;
}

/// Allocation statistics for monitoring.
#[derive(Debug, Clone, Copy)]
pub struct AllocStats {
    /// Total allocations performed
    pub alloc_count: u64,
    /// Total deallocations performed
    pub dealloc_count: u64,
    /// Current bytes allocated
    pub bytes_allocated: usize,
    /// Peak bytes allocated
    pub peak_bytes: usize,
    /// Number of allocation failures
    pub failed_allocs: u64,
}

/// Allocator with statistics tracking.
pub trait ViStatAllocator: ViGlobalAllocator {
    /// Get allocation statistics.
    fn stats(&self) -> AllocStats;

    /// Reset statistics counters.
    fn reset_stats(&mut self);
}
