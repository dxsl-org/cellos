//! RISC-V IOMMU Sv39 page table (3 levels), one per Cell domain.
//!
//! Identity leaves (IOVA == PA) for granted DMA ranges. Any other IOVA faults
//! at the IOMMU.

use crate::dev::iommu_pt::{alloc_page, phys_to_virt_inner};

const SV39_V: u64 = 1 << 0; // Valid
const SV39_R: u64 = 1 << 1; // Read
const SV39_W: u64 = 1 << 2; // Write
const SV39_A: u64 = 1 << 6; // Accessed (pre-set to avoid IOMMU A-fault on first DMA)
const SV39_D: u64 = 1 << 7; // Dirty   (pre-set to avoid IOMMU D-fault on first DMA)
/// Leaf DMA PTE: V|R|W|A|D — readable+writable, no execute.
const PTE_DMA: u64 = SV39_V | SV39_R | SV39_W | SV39_A | SV39_D;

/// Sv39 3-level identity-mapping page table for RISC-V IOMMU second-stage.
pub struct Sv39IommuPt {
    root_phys: u64,
    root_virt: usize,
}

// SAFETY: pointers are into kernel-owned 4 KiB pages never aliased externally.
unsafe impl Send for Sv39IommuPt {}

impl Default for Sv39IommuPt {
    fn default() -> Self {
        Self::new()
    }
}

impl Sv39IommuPt {
    pub fn new() -> Self {
        #[cfg(feature = "p01-trace")]
        let _p01_activity = crate::p01_noncpu::activity();
        let (v, p) = alloc_page();
        #[cfg(feature = "p01-trace")]
        crate::p01_noncpu::created_heap(p, 3, v);
        Self {
            root_phys: p,
            root_virt: v,
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
        let vpn2 = ((pa >> 30) & 0x1FF) as usize;
        let vpn1 = ((pa >> 21) & 0x1FF) as usize;
        let vpn0 = ((pa >> 12) & 0x1FF) as usize;
        let l1_phys = ensure_sv39_child(self.root_virt, vpn2);
        let l0_phys = ensure_sv39_child(phys_to_virt_inner(l1_phys), vpn1);
        let leaf = ((pa >> 12) << 10) | PTE_DMA;
        // SAFETY: l0 page is a valid 4 KiB allocation; vpn0 < 512.
        unsafe {
            let ptr = (phys_to_virt_inner(l0_phys) + vpn0 * 8) as *mut u64;
            ptr.write_volatile(leaf);
        }
    }

    /// Remove the identity mapping for `[phys, phys+size)`. Idempotent.
    ///
    /// Lookup-only: a missing child table is skipped rather than allocated, and
    /// no intermediate table is ever freed — a sibling range sharing the same
    /// 4 KiB table may still be mapped. Returns the number of leaf entries
    /// cleared, so a caller can tell "nothing was mapped" from "unmapped".
    ///
    /// Zeroing a leaf is not enough on its own: the caller must invalidate the
    /// IOMMU's cached translation for the cleared page before the frames behind
    /// it are reused.
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
        let vpn2 = ((pa >> 30) & 0x1FF) as usize;
        let vpn1 = ((pa >> 21) & 0x1FF) as usize;
        let vpn0 = ((pa >> 12) & 0x1FF) as usize;
        let Some(l1_phys) = lookup_sv39_child(self.root_virt, vpn2) else {
            return false;
        };
        let Some(l0_phys) = lookup_sv39_child(phys_to_virt_inner(l1_phys), vpn1) else {
            return false;
        };
        // SAFETY: l0 is a valid 4 KiB table reached through valid entries; vpn0 < 512.
        let slot = (phys_to_virt_inner(l0_phys) + vpn0 * 8) as *mut u64;
        unsafe {
            if slot.read_volatile() == 0 {
                return false;
            }
            slot.write_volatile(0);
        }
        true
    }

    /// Physical address of the root page (program into Device Context satp.PPN).
    #[inline]
    pub fn root_phys(&self) -> u64 {
        self.root_phys
    }
}

/// Get or allocate a child table at `table_virt[idx]`. Returns child phys.
fn ensure_sv39_child(table_virt: usize, idx: usize) -> u64 {
    // SAFETY: table_virt is a 512-entry 4 KiB page; idx < 512.
    let slot = (table_virt + idx * 8) as *mut u64;
    let e = unsafe { slot.read_volatile() };
    if e & SV39_V != 0 {
        return (e >> 10) << 12; // extract PPN → phys
    }
    let (_, child_phys) = alloc_page();
    let ptr_pte = ((child_phys >> 12) << 10) | SV39_V; // V=1, no R/W/X = non-leaf
    unsafe {
        slot.write_volatile(ptr_pte);
    }
    child_phys
}

/// Child table at `table_virt[idx]`, or `None` when the entry is not valid.
///
/// The teardown counterpart of [`ensure_sv39_child`]: never allocates.
fn lookup_sv39_child(table_virt: usize, idx: usize) -> Option<u64> {
    // SAFETY: table_virt is a 512-entry 4 KiB page; idx < 512.
    let e = unsafe { ((table_virt + idx * 8) as *const u64).read_volatile() };
    if e & SV39_V != 0 {
        Some((e >> 10) << 12)
    } else {
        None
    }
}

/// The exact leaf an IOMMU walk would consult for `pa`.
#[cfg(feature = "test-hooks")]
impl Sv39IommuPt {
    pub(crate) fn leaf_present(&self, pa: u64) -> bool {
        let vpn2 = ((pa >> 30) & 0x1FF) as usize;
        let vpn1 = ((pa >> 21) & 0x1FF) as usize;
        let vpn0 = ((pa >> 12) & 0x1FF) as usize;
        let Some(l1_phys) = lookup_sv39_child(self.root_virt, vpn2) else {
            return false;
        };
        let Some(l0_phys) = lookup_sv39_child(phys_to_virt_inner(l1_phys), vpn1) else {
            return false;
        };
        // SAFETY: l0 is a valid 4 KiB table reached through valid entries; vpn0 < 512.
        unsafe { ((phys_to_virt_inner(l0_phys) + vpn0 * 8) as *const u64).read_volatile() != 0 }
    }

    /// Whether the level-2 table covering `pa` exists at all.
    pub(crate) fn level2_present(&self, pa: u64) -> bool {
        lookup_sv39_child(self.root_virt, ((pa >> 30) & 0x1FF) as usize).is_some()
    }
}

/// [`crate::dev::iommu_pt::probe_teardown`] on a fresh table no IOMMU walks.
#[cfg(feature = "test-hooks")]
pub(super) fn teardown_probe() -> crate::dev::iommu_pt::TeardownProbe {
    let sv39 = Sv39IommuPt::new();
    crate::dev::iommu_pt::probe_teardown(
        |phys, size| sv39.map_range(phys, size),
        |phys, size| sv39.unmap_range(phys, size),
        |pa| sv39.leaf_present(pa),
        |pa| sv39.level2_present(pa),
    )
}
