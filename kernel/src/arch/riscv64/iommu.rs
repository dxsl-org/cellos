// SPDX-License-Identifier: MPL-2.0
//! `iommu`: the riscv64 arms of `crate::dev::iommu`'s dispatch (RISC-V IOMMU)
//! and of `crate::dev::iommu_pt`'s address translation.

use crate::dev::iommu::{DmaIsolationInput, DmaMapResult};

/// Phase 1 of [`crate::dev::iommu::init`].
pub fn init(dma: DmaIsolationInput) {
    let _ = dma;
    crate::arch::dev::iommu_riscv::init_hw();
}

/// [`crate::dev::iommu::map_dma_for_cell`] while isolation is not active.
pub fn map_inactive(tid: u64, bdf: u32, phys: u64, size: usize) -> DmaMapResult {
    let _ = (tid, bdf, phys, size);
    DmaMapResult::Rejected
}

/// [`crate::dev::iommu::map_dma_for_cell`] once isolation is active.
pub fn map_range_for_cell(tid: u64, bdf: u32, phys: u64, size: usize) -> DmaMapResult {
    crate::arch::dev::iommu_riscv::map_range_for_cell(tid, bdf, phys, size)
}

/// [`crate::dev::iommu::revoke_dma_for_cell`].
pub fn revoke_cell(tid: u64) -> bool {
    crate::arch::dev::iommu_riscv::unmap_cell(tid)
}

/// Phase 3 of [`crate::dev::iommu::activate_isolation`].
pub fn activate() {
    crate::arch::dev::iommu_riscv::activate();
}

/// Whether [`init`] found and set up the RISC-V IOMMU.
pub fn is_present() -> bool {
    crate::arch::dev::iommu_riscv::is_present()
}

/// Physical address of a kernel-heap pointer (identity).
#[inline]
pub(crate) fn virt_to_phys(virt: usize) -> u64 {
    virt as u64
}

/// Kernel-heap pointer of a physical address (identity).
#[inline]
pub(crate) fn phys_to_virt(phys: u64) -> usize {
    phys as usize
}

/// IOMMU table write-back: VT-d exists only on x86_64; elsewhere the table is
/// a host-side probe.
#[inline]
pub(crate) fn sync_table(virt: usize, len: usize, coherent: bool) {
    let _ = (virt, len, coherent);
}
