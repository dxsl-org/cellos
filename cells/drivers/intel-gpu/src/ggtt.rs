#![allow(dead_code, unused_imports)]
//! Global Graphics Translation Table (GGTT) management for Intel Xe integrated GPUs.
//!
//! Upstream reference: `drivers/gpu/drm/xe/xe_ggtt.c` and `xe_gtt_defs.h` (MIT).
//!
//! GGTT maps system memory into the 32-bit (4 GiB) global aperture used by
//! the display engine (when enabled), GuC firmware DMA, and CT communication buffers.
//! Each PTE is 64 bits (8 bytes) mapping a 4 KiB page.

use crate::regs::RegisterFile;
use intel_gpu_core::pte::{PatIndex, ADDR_MASK_4K, PAGE_PRESENT, PAGE_RW};
use types::{ViError, ViResult};

pub const GGTT_PAGE_SIZE: usize = 4096;
pub const GGTT_PTE_SIZE: usize = 8;
pub const DEFAULT_GGTT_SIZE: u64 = 4 * 1024 * 1024 * 1024; // 4 GiB

/// Encode a single 64-bit GGTT PTE entry.
pub fn encode_ggtt_pte(phys: u64, writable: bool, pat: PatIndex) -> ViResult<u64> {
    if phys & !ADDR_MASK_4K != 0 {
        return Err(ViError::InvalidArgument);
    }
    let mut pte = phys | PAGE_PRESENT;
    if writable {
        pte |= PAGE_RW;
    }
    // Set PAT index bits (on Xe2/Xe3, PAT0 = bit 3, PAT1 = bit 4)
    let pat_val = pat.get();
    if pat_val & (1 << 0) != 0 {
        pte |= 1 << 3;
    }
    if pat_val & (1 << 1) != 0 {
        pte |= 1 << 4;
    }
    Ok(pte)
}

/// Simple bump/range allocator for GGTT addresses.
pub struct GgttAllocator {
    pub base: u64,
    pub size: u64,
    pub next_offset: u64,
}

impl GgttAllocator {
    pub fn new(base: u64, size: u64) -> Self {
        Self {
            base,
            size,
            next_offset: base,
        }
    }

    /// Allocate a contiguous range of GGTT address space aligned to 4 KiB.
    pub fn allocate(&mut self, bytes: usize) -> ViResult<u64> {
        if bytes == 0 {
            return Err(ViError::InvalidArgument);
        }
        let aligned_bytes = (bytes + GGTT_PAGE_SIZE - 1) & !(GGTT_PAGE_SIZE - 1);
        let current = self.next_offset;
        let end = match current.checked_add(aligned_bytes as u64) {
            Some(e) => e,
            None => return Err(ViError::OutOfMemory),
        };
        if end > self.base.saturating_add(self.size) {
            return Err(ViError::OutOfMemory);
        }
        self.next_offset = end;
        Ok(current)
    }
}
