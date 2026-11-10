//! DMA buffer helpers for Tier-1 Driver Cells.
//!
//! Driver Cells allocate physically contiguous memory for DMA rings via `sys_grant_alloc`
//! (which uses `allocate_contiguous` internally), then authorise the IOMMU via
//! `sys_grant_dma`. This module wraps both into a safe interface.
//!
//! # Usage
//! ```ignore
//! let buf = DmaBuf::alloc(4).expect("OOM");       // 4 pages (16 KiB)
//! let iova = buf.authorize(0x0300).expect("IOMMU deny");
//! // Program `iova` into device registers/descriptors; use `virt()` for CPU access.
//! # let _ = iova;

use crate::syscall::{
    sys_export_dma_range, sys_grant_alloc, sys_grant_cache_sync_begin,
    sys_grant_cache_sync_complete, sys_grant_dma, sys_grant_free, DmaExportError, SyscallError,
};

/// A physically-contiguous, page-aligned DMA buffer backed by a Grant region.
///
/// In SAS the CPU virtual and physical addresses are currently identical.
/// Device-visible addresses are established by [`DmaBuf::authorize`] and may
/// differ from both; callers must program the returned IOVA into hardware.
///
/// Drop does NOT free the buffer — call [`DmaBuf::free`] explicitly.
/// Driver Cells typically hold DMA buffers for their entire lifetime.
pub struct DmaBuf {
    grant_id: usize, // physical base == virtual base in SAS
    n_pages: usize,
}

impl DmaBuf {
    /// Allocate `n_pages` contiguous 4-KiB pages suitable for DMA.
    pub fn alloc(n_pages: usize) -> Option<Self> {
        let grant_id = sys_grant_alloc(n_pages * 4096)?;
        Some(Self { grant_id, n_pages })
    }

    /// CPU physical base address; do not substitute this for an authorized IOVA.
    #[inline]
    pub fn phys(&self) -> usize {
        self.grant_id
    }

    /// Virtual address — identical to `phys()` in SAS.
    #[inline]
    pub fn virt(&self) -> *mut u8 {
        self.grant_id as *mut u8
    }

    /// Total size in bytes.
    #[inline]
    pub fn size(&self) -> usize {
        self.n_pages * 4096
    }

    /// Authorise DMA for the PCIe device at `bdf` covering this buffer.
    ///
    /// Must be called before issuing any DMA operation.  The IOMMU maps
    /// `[phys, phys + size)` for this BDF in the calling cell's domain.
    /// Returns `Ok(iova)` on success (`iova == phys` in SAS).
    pub fn authorize(&self, bdf: u32) -> Result<u64, SyscallError> {
        sys_grant_dma(bdf, self.grant_id as u64, self.size())
    }

    /// Begin a cache-coherent device operation over a bounded subrange.
    pub fn begin_cache_sync(&self, offset: usize, len: usize) -> Option<usize> {
        sys_grant_cache_sync_begin(self.grant_id, offset, len)
    }

    /// Complete a previously started cache synchronization operation.
    pub fn complete_cache_sync(&self, token: usize) -> bool {
        sys_grant_cache_sync_complete(token)
    }

    /// Export `[offset, offset + len)` of this buffer to the live provider of
    /// `holder_service` (`ExportDmaRange = 437`, ADR-0033). The holder maps it
    /// with [`map_export`] and its device reaches it through a kernel-chosen
    /// IOVA window; `device_write` false makes the window device-read-only.
    /// The buffer cannot be freed until the export is revoked ([`revoke_export`]).
    pub fn export(
        &self,
        offset: usize,
        len: usize,
        holder_service: u16,
        device_write: bool,
    ) -> Result<u64, DmaExportError> {
        if offset.checked_add(len).is_none_or(|end| end > self.size()) {
            return Err(DmaExportError::InvalidInput);
        }
        let flags = if device_write {
            api::dma_export::EXPORT_DEVICE_WRITE
        } else {
            0
        };
        sys_export_dma_range(self.grant_id + offset, len, holder_service, flags)
    }

    /// Copy `src` into the buffer at `offset`. Returns `false` (and copies
    /// nothing) when the span does not fit.
    pub fn write_bytes(&self, offset: usize, src: &[u8]) -> bool {
        if offset
            .checked_add(src.len())
            .is_none_or(|end| end > self.size())
        {
            return false;
        }
        for (i, byte) in src.iter().enumerate() {
            // SAFETY: the span lies inside this live, caller-owned Grant, which
            // is mapped USER RW at `grant_id` for the buffer's whole size.
            // Volatile: a device may read it through an authorized window.
            unsafe { self.virt().add(offset + i).write_volatile(*byte) };
        }
        true
    }

    /// Copy `dst.len()` bytes at `offset` out of the buffer. Returns `false`
    /// (and copies nothing) when the span does not fit.
    pub fn read_bytes(&self, offset: usize, dst: &mut [u8]) -> bool {
        if offset
            .checked_add(dst.len())
            .is_none_or(|end| end > self.size())
        {
            return false;
        }
        for (i, byte) in dst.iter_mut().enumerate() {
            // SAFETY: as in `write_bytes`; volatile because a device may have
            // written the memory behind the compiler's back.
            *byte = unsafe { self.virt().add(offset + i).read_volatile() };
        }
        true
    }

    /// Release the Grant region and return frames to the kernel.
    pub fn free(self) {
        sys_grant_free(self.grant_id);
    }
}

/// 438: map a client's export into this Driver Cell's VT-d domain for `bdf`.
pub use crate::syscall::sys_map_dma_export as map_export;
pub use crate::syscall::sys_register_nic_driver as register_nic_driver;
/// 439: revoke an export (owner or holder).
pub use crate::syscall::sys_revoke_dma_export as revoke_export;
