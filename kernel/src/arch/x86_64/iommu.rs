// SPDX-License-Identifier: MPL-2.0
//! `iommu`: the x86_64 arms of `crate::dev::iommu`'s dispatch (Intel VT-d) and
//! of `crate::dev::iommu_pt`'s address translation.

use crate::dev::iommu::{
    dma_without_remapper, DmaIsolationInput, DmaMapResult, DmaUnmapResult, DmaWithoutRemapper,
};
use core::sync::atomic::{AtomicBool, Ordering};

/// Set once when a machine without a remapper is allowed identity DMA, so the
/// condition appears in the boot log without repeating on every buffer.
static IDENTITY_DMA_LOGGED: AtomicBool = AtomicBool::new(false);

/// Phase 1 of [`crate::dev::iommu::init`].
pub fn init(dma: DmaIsolationInput) {
    crate::arch::dev::iommu_x86::init_hw(dma);
}

/// [`crate::dev::iommu::map_dma_for_cell`] while isolation is not active.
pub fn map_inactive(tid: u64, bdf: u32, phys: u64, size: usize) -> DmaMapResult {
    if crate::arch::dev::iommu_x86::is_present() {
        // Present but not enforcing yet: fail closed (unchanged).
        DmaMapResult::Rejected
    } else {
        let board = crate::board::selected();
        match dma_without_remapper(board.dma_isolation) {
            DmaWithoutRemapper::Refuse => {
                log::error!(
                    "[iommu] DMA isolation REQUIRED by board {} but no remapper was \
                     discovered — refusing untranslated DMA (tid={} bdf={:#x} \
                     phys={:#x} size={})",
                    board.slug,
                    tid,
                    bdf,
                    phys,
                    size
                );
                DmaMapResult::Rejected
            }
            DmaWithoutRemapper::Identity => {
                if !IDENTITY_DMA_LOGGED.swap(true, Ordering::Relaxed) {
                    log::warn!(
                        "[iommu] board {} declares no DMA remapper; DMA is untranslated \
                         (identity addressing)",
                        board.slug
                    );
                }
                DmaMapResult::Mapped(phys)
            }
        }
    }
}

/// [`crate::dev::iommu::map_dma_for_cell`] once isolation is active.
pub fn map_range_for_cell(tid: u64, bdf: u32, phys: u64, size: usize) -> DmaMapResult {
    crate::arch::dev::iommu_x86::map_range_for_cell(tid, bdf, phys, size)
}

/// [`crate::dev::iommu::unmap_dma`].
pub fn unmap_range_for_cell(tid: u64, iova: u64, size: usize) -> DmaUnmapResult {
    crate::arch::dev::iommu_x86::unmap_range_for_cell(tid, iova, size)
}

/// [`crate::dev::iommu::revoke_dma_for_cell`].
pub fn revoke_cell(tid: u64) -> bool {
    crate::arch::dev::iommu_x86::unmap_cell_domain(tid)
}

/// Phase 3 of [`crate::dev::iommu::activate_isolation`].
pub fn activate() {
    crate::arch::dev::iommu_x86::activate();
}

/// Physical address of a kernel-heap pointer (HHDM).
#[inline]
pub(crate) fn virt_to_phys(virt: usize) -> u64 {
    (virt - crate::memory::frame::phys_to_virt(0)) as u64
}

/// Kernel-heap pointer of a physical address (HHDM).
#[inline]
pub(crate) fn phys_to_virt(phys: u64) -> usize {
    phys as usize + crate::memory::frame::phys_to_virt(0)
}

/// Write `[virt, virt + len)` of an IOMMU table back to memory for a
/// non-snooping unit.
#[inline]
pub(crate) fn sync_table(virt: usize, len: usize, coherent: bool) {
    if !coherent {
        crate::arch::dev::iommu_x86::cache::flush(virt, len);
    }
}
