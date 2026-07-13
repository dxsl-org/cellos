//! DMA buffer helpers for Tier-1 Driver Cells.
//!
//! Driver Cells allocate physically contiguous memory for DMA rings via `sys_grant_alloc`
//! (which uses `allocate_contiguous` internally), then authorise the IOMMU via
//! `sys_grant_dma`. This module wraps both into a safe interface.
//!
//! # Usage
//! ```ignore
//! let buf = DmaBuf::alloc(4).expect("OOM");      // 4 pages (16 KiB)
//! buf.authorize(0x0300).expect("IOMMU deny");     // BDF 0:3:0
//! let phys = buf.phys();                         // program into NVMe/e1000 regs
//! ```

use crate::syscall::{sys_grant_alloc, sys_grant_dma, sys_grant_free, SyscallError};

/// A physically-contiguous, page-aligned DMA buffer backed by a Grant region.
///
/// In SAS the virtual address equals the physical address (identity mapping),
/// so [`phys`](DmaBuf::phys) and [`virt`](DmaBuf::virt) return the same value.
///
/// Drop does NOT free the buffer — call [`DmaBuf::free`] explicitly.
/// Driver Cells typically hold DMA buffers for their entire lifetime.
pub struct DmaBuf {
    grant_id: usize, // physical base == virtual base in SAS
    n_pages:  usize,
}

impl DmaBuf {
    /// Allocate `n_pages` contiguous 4-KiB pages suitable for DMA.
    pub fn alloc(n_pages: usize) -> Option<Self> {
        let grant_id = sys_grant_alloc(n_pages * 4096)?;
        Some(Self { grant_id, n_pages })
    }

    /// Physical base address — program this into DMA descriptor registers.
    #[inline]
    pub fn phys(&self) -> usize { self.grant_id }

    /// Virtual address — identical to `phys()` in SAS.
    #[inline]
    pub fn virt(&self) -> *mut u8 { self.grant_id as *mut u8 }

    /// Total size in bytes.
    #[inline]
    pub fn size(&self) -> usize { self.n_pages * 4096 }

    /// Authorise DMA for the PCIe device at `bdf` covering this buffer.
    ///
    /// Must be called before issuing any DMA operation.  The IOMMU maps
    /// `[phys, phys + size)` for this BDF in the calling cell's domain.
    /// Returns `Ok(iova)` on success (`iova == phys` in SAS).
    pub fn authorize(&self, bdf: u32) -> Result<u64, SyscallError> {
        sys_grant_dma(bdf, self.grant_id as u64, self.size())
    }

    /// Release the Grant region and return frames to the kernel.
    pub fn free(self) {
        sys_grant_free(self.grant_id);
    }
}

pub use crate::syscall::{sys_register_block_driver as register_block_driver,
                         sys_register_nic_driver   as register_nic_driver};
