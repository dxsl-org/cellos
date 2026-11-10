//! Global Graphics Translation Table (GGTT) management for Intel Xe integrated GPUs.
//!
//! Upstream reference: `drivers/gpu/drm/xe/xe_ggtt.c`, `xe_wopcm.c`, `xe_gtt_defs.h` and
//! `xe_guc_tlb_inval.c` (MIT).
//!
//! The GGTT maps device-visible memory into the 32-bit global aperture the GuC and its
//! firmware DMA use. Each 64-bit entry maps one 4 KiB page; the entries live in the GTT
//! stolen memory (GSM) that graphics IP 12.50 and later expose 8 MiB into BAR0.

use crate::regs::RegisterFile;
use intel_gpu_core::pte::{PatIndex, ADDR_MASK_4K, PAGE_PRESENT};
use types::{ViError, ViResult};

pub const GGTT_PAGE_SIZE: usize = 4096;
/// Byte offset of the GSM (the GGTT entries) inside BAR0 (`ggtt->gsm`).
pub const GSM_OFFSET: usize = 0x80_0000;
/// GSM size on IP 12.50+: 8 MiB of entries, a 4 GiB GGTT.
pub const GSM_LEN: usize = 0x80_0000;
/// `LNL_WOPCM_SIZE`: from Lunar Lake on the GGTT below 8 MiB is WOPCM, never allocated.
pub const WOPCM_SIZE: u64 = 8 << 20;

/// `XELPG_GGTT_PTE_PAT0`/`PAT1`: GGTT PAT bits sit at 53:52, not at the PPGTT positions.
pub const GGTT_PTE_PAT0: u64 = 1 << 52;
pub const GGTT_PTE_PAT1: u64 = 1 << 53;

/// `PVC_GUC_TLB_INV_DESC0`/`DESC1`: the MMIO GGTT TLB invalidation graphics version 20
/// and later use while the GuC is not yet running.
pub const GUC_TLB_INV_DESC0: usize = 0x00CF7C;
pub const GUC_TLB_INV_DESC0_VALID: u32 = 1 << 0;
pub const GUC_TLB_INV_DESC1: usize = 0x00CF80;
pub const GUC_TLB_INV_DESC1_INVALIDATE: u32 = 1 << 6;

/// Encode a 64-bit GGTT PTE for system memory (`xelpg_ggtt_pte_flags`).
///
/// GGTT entries have no write-enable bit: bit 1 is `XE_GGTT_PTE_DM` (device memory) and must
/// stay clear on an integrated GPU. Only PAT indices 0..=3 are encodable.
pub fn encode_ggtt_pte(phys: u64, pat: PatIndex) -> ViResult<u64> {
    if phys & !ADDR_MASK_4K != 0 || pat.get() > 3 {
        return Err(ViError::InvalidArgument);
    }
    let mut pte = phys | PAGE_PRESENT;
    if pat.get() & 1 != 0 {
        pte |= GGTT_PTE_PAT0;
    }
    if pat.get() & 2 != 0 {
        pte |= GGTT_PTE_PAT1;
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
        let aligned_bytes = bytes
            .checked_next_multiple_of(GGTT_PAGE_SIZE)
            .ok_or(ViError::OutOfMemory)?;
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

/// The device's GGTT: address allocation plus the entry writes into GSM.
pub struct Ggtt {
    alloc: GgttAllocator,
}

impl Default for Ggtt {
    fn default() -> Self {
        Self::new()
    }
}

impl Ggtt {
    /// The whole GSM-backed range above WOPCM (`xe_ggtt_init_early`).
    pub fn new() -> Self {
        let top = (GSM_LEN / 8 * GGTT_PAGE_SIZE) as u64;
        Self {
            alloc: GgttAllocator::new(WOPCM_SIZE, top - WOPCM_SIZE),
        }
    }

    /// Map `len` bytes of device memory at `iova` (page aligned) into a fresh GGTT range
    /// and invalidate the GGTT TLBs. Returns the GGTT address of `iova`.
    pub fn map(
        &mut self,
        regs: &mut impl RegisterFile,
        iova: u64,
        len: usize,
        pat: PatIndex,
    ) -> ViResult<u64> {
        if !iova.is_multiple_of(GGTT_PAGE_SIZE as u64) {
            return Err(ViError::InvalidArgument);
        }
        let addr = self.alloc.allocate(len)?;
        let first = (addr / GGTT_PAGE_SIZE as u64) as usize;
        for page in 0..len.div_ceil(GGTT_PAGE_SIZE) {
            let phys = iova
                .checked_add((page * GGTT_PAGE_SIZE) as u64)
                .ok_or(ViError::InvalidArgument)?;
            regs.write64(GSM_OFFSET + (first + page) * 8, encode_ggtt_pte(phys, pat)?)?;
        }
        invalidate(regs)?;
        Ok(addr)
    }
}

/// `send_tlb_inval_ggtt` MMIO path for graphics version 20+ (GuC not yet communicating).
pub fn invalidate(regs: &mut impl RegisterFile) -> ViResult<()> {
    regs.write32(GUC_TLB_INV_DESC1, GUC_TLB_INV_DESC1_INVALIDATE)?;
    regs.write32(GUC_TLB_INV_DESC0, GUC_TLB_INV_DESC0_VALID)
}
