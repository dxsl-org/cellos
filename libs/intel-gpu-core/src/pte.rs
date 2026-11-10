//! Xe2/Xe3 PPGTT entry encoding: `drivers/gpu/drm/xe/regs/xe_gtt_defs.h` and
//! `pde_encode_pat_index` / `pte_encode_pat_index` / `pte_encode_ps` /
//! `xelp_pte_encode_addr` in `drivers/gpu/drm/xe/xe_vm.c` (all MIT).

pub const PAGE_PRESENT: u64 = 1 << 0;
pub const PAGE_RW: u64 = 1 << 1;
pub const PPGTT_PTE_PAT0: u64 = 1 << 3;
pub const PPGTT_PTE_PAT1: u64 = 1 << 4;
/// PAT bit 2 in a level-0 PTE.
pub const PPGTT_PTE_PAT2: u64 = 1 << 7;
/// Page-size bit: 2 MiB leaf at level 1, 1 GiB leaf at level 2 (same bit).
pub const PDE_PS_LEAF: u64 = 1 << 7;
pub const PTE_PS64: u64 = 1 << 8;
/// `XE_PTE_NULL`: a present leaf with no backing page; reads return zero, writes drop.
pub const XE_PTE_NULL: u64 = 1 << 9;
/// Device-memory bit; never set for an integrated GPU's system memory.
pub const PPGTT_PTE_DM: u64 = 1 << 11;
/// PAT bit 2 in a level-1/level-2 leaf.
pub const PPGTT_PDE_PDPE_PAT2: u64 = 1 << 12;
pub const XE2_PPGTT_PTE_PAT4: u64 = 1 << 61;
pub const XELPG_PPGTT_PTE_PAT3: u64 = 1 << 62;

/// Address bits 51:12 (`XE_PAGE_ADDR_MASK(12)`).
pub const ADDR_MASK_4K: u64 = ((1 << 52) - 1) & !((1 << 12) - 1);

/// PAT indices of the Xe2 and Xe3-LPG tables (`xe_pat.c`, graphics 20/30).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PatIndex(u8);

impl PatIndex {
    /// `XE_CACHE_WB`: write-back, coherent with the CPU.
    pub const WB: Self = Self(2);
    /// `XE_CACHE_NONE`: uncached.
    pub const UC: Self = Self(3);
    /// `XE_CACHE_WT`.
    pub const WT: Self = Self(15);

    /// An index from the 32-entry table; `None` outside it.
    pub const fn new(index: u8) -> Option<Self> {
        if index < 32 {
            Some(Self(index))
        } else {
            None
        }
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodeError {
    /// Address not aligned to the entry's page size.
    Misaligned,
    /// Address bits beyond 51 set, or beyond the device DMA width.
    AddressTooWide,
    /// Leaf requested above level 2 (`MAX_HUGEPTE_LEVEL`).
    LevelHasNoLeaf,
    /// A non-leaf entry can encode only PAT indices 0..=3.
    PatTooWideForPde,
}

/// Bytes mapped by one entry at `level` with a 4 KiB base page.
pub const fn level_span(level: u32) -> u64 {
    1u64 << (12 + 9 * level)
}

/// Encode a leaf mapping `phys` at `level` (0 = 4 KiB, 1 = 2 MiB, 2 = 1 GiB).
///
/// `dma_bits` is the device DMA width (46 on Panther Lake, `ptl_desc.dma_mask_size`).
pub fn encode_leaf(
    phys: u64,
    level: u32,
    pat: PatIndex,
    writable: bool,
    dma_bits: u32,
) -> Result<u64, EncodeError> {
    if level > 2 {
        return Err(EncodeError::LevelHasNoLeaf);
    }
    if !phys.is_multiple_of(level_span(level)) {
        return Err(EncodeError::Misaligned);
    }
    if (dma_bits < 64 && phys >> dma_bits != 0) || phys & !ADDR_MASK_4K != 0 {
        return Err(EncodeError::AddressTooWide);
    }
    let mut pte = phys | PAGE_PRESENT;
    if writable {
        pte |= PAGE_RW;
    }
    pte |= pat_bits(pat.get(), level);
    if level > 0 {
        pte |= PDE_PS_LEAF;
    }
    Ok(pte)
}

/// Encode a 4 KiB `NULL` leaf the way `xelp_pte_encode_vma` does for a NULL VMA: address
/// 0, present, writable (writes are dropped by the hardware), the PAT bits, `XE_PTE_NULL`.
pub fn encode_null_leaf(pat: PatIndex) -> u64 {
    PAGE_PRESENT | PAGE_RW | pat_bits(pat.get(), 0) | XE_PTE_NULL
}

/// Encode a non-leaf entry pointing at the next-level table at `table_phys`.
pub fn encode_table(table_phys: u64, pat: PatIndex, dma_bits: u32) -> Result<u64, EncodeError> {
    if pat.get() > 3 {
        return Err(EncodeError::PatTooWideForPde);
    }
    if !table_phys.is_multiple_of(level_span(0)) {
        return Err(EncodeError::Misaligned);
    }
    if (dma_bits < 64 && table_phys >> dma_bits != 0) || table_phys & !ADDR_MASK_4K != 0 {
        return Err(EncodeError::AddressTooWide);
    }
    let pat_low = pat_bits(pat.get(), 1) & (PPGTT_PTE_PAT0 | PPGTT_PTE_PAT1);
    Ok(table_phys | PAGE_PRESENT | PAGE_RW | pat_low)
}

fn pat_bits(index: u8, level: u32) -> u64 {
    let mut bits = 0;
    if index & 1 != 0 {
        bits |= PPGTT_PTE_PAT0;
    }
    if index & 2 != 0 {
        bits |= PPGTT_PTE_PAT1;
    }
    if index & 4 != 0 {
        bits |= if level > 0 {
            PPGTT_PDE_PDPE_PAT2
        } else {
            PPGTT_PTE_PAT2
        };
    }
    if index & 8 != 0 {
        bits |= XELPG_PPGTT_PTE_PAT3;
    }
    if index & 16 != 0 {
        bits |= XE2_PPGTT_PTE_PAT4;
    }
    bits
}

#[cfg(test)]
mod tests;
