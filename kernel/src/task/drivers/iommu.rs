//! IOMMU common API — bare/passthrough mode dispatcher.
//!
//! Dispatches to the arch-specific backend (`iommu_riscv` or `iommu_x86`).
//! In bare/passthrough mode IOVA == PA, so `map_dma` is a no-op identity
//! function. All call sites are future-proof: switching to Sv39x4 page tables
//! only requires filling in the arch backends, not changing callers.
//!
//! BARE MODE IS NOT SAFE ON REAL HARDWARE — full IOMMU page tables are
//! required before multi-tenant G2.

use core::sync::atomic::{AtomicBool, Ordering};

static IOMMU_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Initialise the arch-specific IOMMU backend.
///
/// Called from `main.rs` after `pcie_ecam::init()` and before NIC/NVMe drivers.
/// Falls through silently when the IOMMU hardware is absent.
pub fn init() {
    #[cfg(target_arch = "riscv64")]
    super::iommu_riscv::init_riscv_iommu();
    #[cfg(target_arch = "x86_64")]
    super::iommu_x86::init_vtd();
}

/// Translate a DMA physical address to an IOVA.
///
/// In bare/passthrough mode: IOVA == phys (identity). In future page-table
/// mode this will allocate an IOMMU mapping and return the assigned IOVA.
#[inline]
pub fn map_dma(phys: u64, _size: usize) -> u64 {
    phys
}

/// Release a previously obtained IOVA.
///
/// No-op in bare/passthrough mode.
#[inline]
pub fn unmap_dma(_iova: u64, _size: usize) {}

/// Returns `true` after a successful `init()` call on the current arch.
#[inline]
pub fn is_active() -> bool {
    IOMMU_ACTIVE.load(Ordering::Relaxed)
}

/// Mark the IOMMU as active. Called by arch backends on successful init.
pub(super) fn set_active() {
    IOMMU_ACTIVE.store(true, Ordering::Relaxed);
}
