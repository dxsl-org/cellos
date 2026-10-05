//! Authorized DMA regions for the xHCI rings, contexts and payload buffers.
//!
//! Every structure the controller fetches by DMA is allocated through
//! `DmaBuf::alloc` and authorized for the controller's BDF before its address is
//! programmed into a register or a context, so phase-05's IOMMU work applies
//! without rework. In SAS the authorized IOVA equals the physical address, but
//! this code only ever programs the returned IOVA.
//!
//! Law 4 exception: Driver Cells may use `unsafe` for DMA memory access; every
//! block carries a `// SAFETY:` comment.

use ostd::dma::DmaBuf;

/// A page-aligned, zeroed, IOMMU-authorized DMA allocation.
pub struct DmaRegion {
    buf: DmaBuf,
    iova: u64,
}

impl DmaRegion {
    /// Allocate `pages` contiguous pages, authorize them for `bdf`, and zero.
    ///
    /// Returns `None` on allocation or authorization failure; never panics.
    pub fn new(pages: usize, bdf: u32) -> Option<Self> {
        let buf = DmaBuf::alloc(pages)?;
        let iova = buf.authorize(bdf).ok()?;
        let region = Self { buf, iova };
        region.zero();
        Some(region)
    }

    #[inline]
    pub fn iova(&self) -> u64 {
        self.iova
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.buf.size()
    }

    pub fn zero(&self) {
        // SAFETY: `buf` is a live grant of `len()` bytes owned by this region;
        // `virt()` is its CPU mapping and `write_bytes` covers exactly that range.
        unsafe { core::ptr::write_bytes(self.buf.virt(), 0, self.len()) }
    }

    pub fn read_u32(&self, off: usize) -> u32 {
        assert!(off + 4 <= self.len(), "DmaRegion::read_u32 out of bounds");
        // SAFETY: bounds checked above against the live grant; unaligned access
        // is not possible because every caller passes a 4-byte-aligned offset.
        unsafe { core::ptr::read_volatile(self.buf.virt().add(off) as *const u32) }
    }

    pub fn write_u32(&self, off: usize, value: u32) {
        assert!(off + 4 <= self.len(), "DmaRegion::write_u32 out of bounds");
        // SAFETY: bounds checked above; the grant is a live, writable CPU mapping.
        unsafe { core::ptr::write_volatile(self.buf.virt().add(off) as *mut u32, value) }
    }

    pub fn read_u64(&self, off: usize) -> u64 {
        assert!(off + 8 <= self.len(), "DmaRegion::read_u64 out of bounds");
        // SAFETY: bounds checked above against the live grant.
        unsafe { core::ptr::read_volatile(self.buf.virt().add(off) as *const u64) }
    }

    pub fn write_u64(&self, off: usize, value: u64) {
        assert!(off + 8 <= self.len(), "DmaRegion::write_u64 out of bounds");
        // SAFETY: bounds checked above; the grant is a live, writable CPU mapping.
        unsafe { core::ptr::write_volatile(self.buf.virt().add(off) as *mut u64, value) }
    }

    pub fn read_bytes(&self, off: usize, out: &mut [u8]) {
        assert!(
            off + out.len() <= self.len(),
            "DmaRegion::read_bytes out of bounds"
        );
        // SAFETY: both ranges are bounds checked above; the source is the live
        // grant, the destination a Rust slice.
        unsafe {
            core::ptr::copy_nonoverlapping(self.buf.virt().add(off), out.as_mut_ptr(), out.len())
        }
    }
}
