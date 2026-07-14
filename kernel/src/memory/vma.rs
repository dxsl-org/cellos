//! Virtual Memory Area — per-cell region tracker for demand-paging.
//!
//! A `VmaList` is embedded in each `Task` (TCB). It records the virtual address
//! ranges that a cell has reserved, together with the backing physical base and
//! the PTE flags to use when a page fault triggers on-demand mapping.
//!
//! In Phase 01, VMA lists are created empty; the ELF loader (Phase 04) populates
//! them with `ElfText` and `ElfData` entries when loading a binary. Until then,
//! any user-mode page fault will find no matching region and panic.

use types::{PhysAddr, VAddr};

/// Semantic classification of a VMA region.
///
/// Used by the kernel to apply appropriate fault-handling policy (e.g., ElfText
/// regions are read-only/executable; Stack regions can grow down).
#[derive(Debug, Clone, PartialEq)]
pub enum VmaKind {
    /// ELF `.text` / read-only executable segment.
    ElfText,
    /// ELF `.data` / `.bss` / read-write segment.
    ElfData,
    /// User-mode stack (grows downward).
    Stack,
    /// Heap (grows upward via `brk`).
    Heap,
    /// Shared Grant region mapped into this cell's address space.
    Grant,
}

/// A single contiguous virtual memory region with a backing physical base.
///
/// `va_start` and `va_end` are page-aligned. `pa_start` is the physical address
/// of the first page of the backing region (for ELF-backed segments; for
/// demand-allocated regions `pa_start` is 0 until allocated).
///
/// `flags` stores raw architecture PTE flags to install on demand-mapping.
/// On x86_64 these are the `PTE_*` constants from `hal::paging`; on RISC-V
/// they are the `PageFlags` bitmask.
#[derive(Debug, Clone)]
pub struct VmaRegion {
    /// Inclusive start of the virtual range (page-aligned).
    pub va_start: VAddr,
    /// Exclusive end of the virtual range (page-aligned).
    pub va_end: VAddr,
    /// Physical base of the backing memory (0 for demand-allocated regions).
    pub pa_start: PhysAddr,
    /// Architecture-specific PTE flags for the on-demand mapping.
    pub flags: u64,
    /// Semantic kind of this region.
    pub kind: VmaKind,
}

/// Per-cell list of virtual memory areas.
///
/// Stored inside `Task` (TCB). All operations are O(n) linear scan — sufficient
/// for the small number of ELF segments + stack + heap per cell.
pub struct VmaList(pub alloc::vec::Vec<VmaRegion>);

impl VmaList {
    /// Create an empty VMA list (zero allocations).
    pub fn new() -> Self {
        Self(alloc::vec::Vec::new())
    }

    /// Append a new region to the list.
    ///
    /// The caller is responsible for ensuring regions are non-overlapping.
    pub fn add(&mut self, r: VmaRegion) {
        self.0.push(r);
    }

    /// Find the region that contains virtual address `va`.
    ///
    /// Returns `None` if no region covers `va`.  Used by the #PF handler to
    /// decide whether a fault is a valid demand-page or a true access violation.
    pub fn find(&self, va: usize) -> Option<&VmaRegion> {
        self.0.iter().find(|r| va >= r.va_start && va < r.va_end)
    }

    /// Remove all regions (called on cell teardown).
    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// Number of registered regions.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns true if no regions have been registered.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Default for VmaList {
    fn default() -> Self {
        Self::new()
    }
}

impl core::fmt::Debug for VmaList {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "VmaList({} regions)", self.0.len())
    }
}
