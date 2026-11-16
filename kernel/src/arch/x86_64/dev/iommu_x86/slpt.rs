//! Intel VT-d second-level page table (3 levels, AW=39-bit).
//!
//! Identity leaves (IOVA == PA) for granted DMA ranges, plus the non-identity
//! export windows of `MapDmaExport`. Any other IOVA faults at the unit.

use crate::dev::iommu_pt::{alloc_page, phys_to_virt_inner};

/// VT-d SLPT entry flag: R=1|W=1 required for all valid entries (leaf + non-leaf).
const VTD_RW: u64 = 0b11;
/// VT-d SLPT leaf permission for a device-read-only export window (`R` only).
const VTD_R: u64 = 0b01;

/// Intel VT-d second-level page table (3 levels, 39-bit address width).
pub struct VtdSlpt {
    root_phys: u64,
    root_virt: usize,
    /// The owning unit snoops CPU caches (`ECAP.C`). When it does not, every
    /// table store must be written back before the unit can see it.
    coherent: bool,
}

// SAFETY: pointers are into kernel-owned 4 KiB pages never aliased externally.
unsafe impl Send for VtdSlpt {}

impl VtdSlpt {
    /// An empty table for a unit whose walks do (`coherent`) or do not snoop
    /// the CPU caches.
    pub fn new(coherent: bool) -> Self {
        #[cfg(feature = "p01-trace")]
        let _p01_activity = crate::p01_noncpu::activity();
        let (v, p) = alloc_page();
        #[cfg(feature = "p01-trace")]
        crate::p01_noncpu::created_heap(p, 4, v);
        let slpt = Self {
            root_phys: p,
            root_virt: v,
            coherent,
        };
        slpt.sync(v, 4096);
        slpt
    }

    /// Write `[virt, virt + len)` back to memory for a non-snooping unit.
    fn sync(&self, virt: usize, len: usize) {
        if !self.coherent {
            super::cache::flush(virt, len);
        }
    }

    /// Add an identity mapping (IOVA == PA) for [phys, phys+size). Idempotent.
    pub fn map_range(&self, phys: u64, size: usize) {
        let start = phys & !0xFFF;
        let end = (phys + size as u64 + 0xFFF) & !0xFFF;
        let mut pa = start;
        while pa < end {
            self.map_page(pa);
            pa += 0x1000;
        }
    }

    fn map_page(&self, pa: u64) {
        self.map_leaf(pa, pa, VTD_RW);
    }

    /// Whether no leaf is present in `[iova, iova + size)`.
    ///
    /// Lookup-only: a missing child table means the whole span under it is
    /// free, and nothing is allocated. Export windows call this before any
    /// change so a window never overwrites an identity or RMRR leaf.
    pub fn window_free(&self, iova: u64, size: usize) -> bool {
        let end = iova + size as u64;
        let mut va = iova & !0xFFF;
        while va < end {
            if self.leaf_at(va).is_some_and(|leaf| leaf != 0) {
                return false;
            }
            va += 0x1000;
        }
        true
    }

    /// Map the physically contiguous `[phys, phys + size)` at the non-identity
    /// IOVA window `[iova, iova + size)` (ADR-0033 CP1 D-G3).
    ///
    /// `writable` clear publishes `R`-only leaves (D-G10), so the remapper
    /// itself refuses device writes. Both bounds must be 4 KiB aligned; the
    /// caller has checked [`Self::window_free`] and invalidates afterwards.
    pub fn map_window(&self, iova: u64, phys: u64, size: usize, writable: bool) {
        let rights = if writable { VTD_RW } else { VTD_R };
        let mut offset = 0u64;
        while offset < size as u64 {
            self.map_leaf(iova + offset, phys + offset, rights);
            offset += 0x1000;
        }
    }

    /// Write one leaf `iova → pa` with `rights`, allocating child tables.
    fn map_leaf(&self, iova: u64, pa: u64, rights: u64) {
        let i2 = ((iova >> 30) & 0x1FF) as usize;
        let i1 = ((iova >> 21) & 0x1FF) as usize;
        let i0 = ((iova >> 12) & 0x1FF) as usize;
        let l1_phys = self.ensure_child(self.root_virt, i2);
        let l0_phys = self.ensure_child(phys_to_virt_inner(l1_phys), i1);
        let ptr = (phys_to_virt_inner(l0_phys) + i0 * 8) as *mut u64;
        // SAFETY: l0 page is a valid 4 KiB allocation; i0 < 512.
        unsafe { ptr.write_volatile((pa & !0xFFF) | rights) };
        self.sync(ptr as usize, 8);
    }

    /// The raw leaf for `iova`, or `None` when a child table is missing.
    fn leaf_at(&self, iova: u64) -> Option<u64> {
        let i2 = ((iova >> 30) & 0x1FF) as usize;
        let i1 = ((iova >> 21) & 0x1FF) as usize;
        let i0 = ((iova >> 12) & 0x1FF) as usize;
        let l1_phys = lookup_vtd_child(self.root_virt, i2)?;
        let l0_phys = lookup_vtd_child(phys_to_virt_inner(l1_phys), i1)?;
        // SAFETY: l0 is a valid 4 KiB table reached through valid entries; i0 < 512.
        Some(unsafe { ((phys_to_virt_inner(l0_phys) + i0 * 8) as *const u64).read_volatile() })
    }

    /// Get or allocate a child table at `table_virt[idx]`. Returns child phys.
    /// A new child is written back (zeroed) before the entry naming it.
    fn ensure_child(&self, table_virt: usize, idx: usize) -> u64 {
        // SAFETY: table_virt is a 512-entry 4 KiB page; idx < 512.
        let slot = (table_virt + idx * 8) as *mut u64;
        let e = unsafe { slot.read_volatile() };
        if e & VTD_RW != 0 {
            return e & !0xFFF; // extract physical address from non-leaf entry
        }
        let (child_virt, child_phys) = alloc_page();
        self.sync(child_virt, 4096);
        // SAFETY: as above.
        unsafe { slot.write_volatile((child_phys & !0xFFF) | VTD_RW) };
        self.sync(slot as usize, 8);
        child_phys
    }

    /// Remove the identity mapping for `[phys, phys+size)`. Idempotent.
    ///
    /// Lookup-only and leaf-only: a missing child table is skipped rather than
    /// allocated, and no intermediate SLPT page is ever freed — a sibling range
    /// sharing the same table may still be mapped. Returns the number of leaf
    /// entries cleared.
    ///
    /// Zeroing a leaf is not enough on its own: the caller must invalidate the
    /// IOTLB entry for the cleared page before the frames behind it are reused
    /// (see `iommu_x86::unmap_window_for_cell`).
    pub fn unmap_range(&self, phys: u64, size: usize) -> usize {
        let start = phys & !0xFFF;
        let end = (phys + size as u64 + 0xFFF) & !0xFFF;
        let mut cleared = 0;
        let mut pa = start;
        while pa < end {
            if self.unmap_page(pa) {
                cleared += 1;
            }
            pa += 0x1000;
        }
        cleared
    }

    /// Zero the leaf entry mapping `pa`. Returns whether one was present.
    fn unmap_page(&self, pa: u64) -> bool {
        let i2 = ((pa >> 30) & 0x1FF) as usize;
        let i1 = ((pa >> 21) & 0x1FF) as usize;
        let i0 = ((pa >> 12) & 0x1FF) as usize;
        let Some(l1_phys) = lookup_vtd_child(self.root_virt, i2) else {
            return false;
        };
        let Some(l0_phys) = lookup_vtd_child(phys_to_virt_inner(l1_phys), i1) else {
            return false;
        };
        // SAFETY: l0 is a valid 4 KiB table reached through valid entries; i0 < 512.
        let slot = (phys_to_virt_inner(l0_phys) + i0 * 8) as *mut u64;
        unsafe {
            if slot.read_volatile() == 0 {
                return false;
            }
            slot.write_volatile(0);
        }
        self.sync(slot as usize, 8);
        true
    }

    /// Physical address of the SLPT root page.
    #[inline]
    pub fn root_phys(&self) -> u64 {
        self.root_phys
    }
}

/// Child table at `table_virt[idx]`, or `None` when the entry is not present.
///
/// The teardown counterpart of [`VtdSlpt::ensure_child`]: never allocates.
fn lookup_vtd_child(table_virt: usize, idx: usize) -> Option<u64> {
    // SAFETY: table_virt is a 512-entry 4 KiB page; idx < 512.
    let e = unsafe { ((table_virt + idx * 8) as *const u64).read_volatile() };
    if e & VTD_RW != 0 {
        Some(e & !0xFFF)
    } else {
        None
    }
}

/// The exact leaf an IOMMU walk would consult for `pa`.
#[cfg(feature = "test-hooks")]
impl VtdSlpt {
    pub(crate) fn leaf_present(&self, pa: u64) -> bool {
        let i2 = ((pa >> 30) & 0x1FF) as usize;
        let i1 = ((pa >> 21) & 0x1FF) as usize;
        let i0 = ((pa >> 12) & 0x1FF) as usize;
        let Some(l1_phys) = lookup_vtd_child(self.root_virt, i2) else {
            return false;
        };
        let Some(l0_phys) = lookup_vtd_child(phys_to_virt_inner(l1_phys), i1) else {
            return false;
        };
        // SAFETY: l0 is a valid 4 KiB table reached through valid entries; i0 < 512.
        unsafe { ((phys_to_virt_inner(l0_phys) + i0 * 8) as *const u64).read_volatile() != 0 }
    }

    /// Whether the level-2 table covering `pa` exists at all.
    pub(crate) fn level2_present(&self, pa: u64) -> bool {
        lookup_vtd_child(self.root_virt, ((pa >> 30) & 0x1FF) as usize).is_some()
    }
}

/// [`crate::dev::iommu_pt::probe_teardown`] on a fresh table no unit walks.
#[cfg(feature = "test-hooks")]
pub(super) fn teardown_probe() -> crate::dev::iommu_pt::TeardownProbe {
    // No unit walks this table; coherent skips the write-back.
    let vtd = VtdSlpt::new(true);
    crate::dev::iommu_pt::probe_teardown(
        |phys, size| vtd.map_range(phys, size),
        |phys, size| vtd.unmap_range(phys, size),
        |pa| vtd.leaf_present(pa),
        |pa| vtd.level2_present(pa),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vtd_slpt_maps_and_unmaps_for_a_non_snooping_unit() {
        // coherent=false writes every table store back with CLFLUSH.
        for coherent in [false, true] {
            let pt = VtdSlpt::new(coherent);
            pt.map_range(0x4020_0000, 0x2000);
            assert_eq!(pt.unmap_range(0x4020_0000, 0x3000), 2);
            assert_eq!(pt.unmap_range(0x4020_0000, 0x2000), 0);
        }
    }

    /// ADR-0033 CP1 D-G3/D-G10: an export window maps non-identity IOVAs in
    /// `[2^38, 2^39)`, `R`-only unless the export grants device writes, never
    /// over a present leaf, and unmaps with the same leaf-only teardown.
    #[test]
    fn vtd_slpt_export_window_is_non_identity_and_rights_exact() {
        const WINDOW: u64 = 1 << 38;
        const PHYS: u64 = 0x4060_0000;
        let pt = VtdSlpt::new(true);
        pt.map_range(PHYS, 0x1000);
        assert!(pt.window_free(WINDOW, 0x3000));
        assert!(!pt.window_free(PHYS, 0x1000), "identity leaf is not free");

        pt.map_window(WINDOW, PHYS, 0x2000, false);
        assert_eq!(pt.leaf_at(WINDOW), Some(PHYS | VTD_R));
        assert_eq!(pt.leaf_at(WINDOW + 0x1000), Some((PHYS + 0x1000) | VTD_R));
        assert!(!pt.window_free(WINDOW + 0x1000, 0x1000));
        assert!(pt.window_free(WINDOW + 0x2000, 0x1000));
        // The identity leaf of the same frame is untouched.
        assert_eq!(pt.leaf_at(PHYS), Some(PHYS | VTD_RW));

        pt.map_window(WINDOW + 0x4000, PHYS, 0x1000, true);
        assert_eq!(pt.leaf_at(WINDOW + 0x4000), Some(PHYS | VTD_RW));

        assert_eq!(pt.unmap_range(WINDOW, 0x5000), 3);
        assert!(pt.window_free(WINDOW, 0x5000));
        assert_eq!(pt.leaf_at(PHYS), Some(PHYS | VTD_RW));
    }
}
