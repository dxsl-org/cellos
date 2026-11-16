//! The cell's one DMA allocation: `DriverShared`, the queue descriptors, the
//! rings and every frame buffer, laid out by [`crate::rings`].
//!
//! Law 4 exception: the volatile accesses below touch device-visible memory.
//! The allocation is owned for the cell's lifetime (the device may hold its
//! addresses until reset), it is authorized for the controller's BDF before
//! any of its addresses is programmed, and every access is bounds-checked
//! against its length; a multi-byte field is aligned by the layout and checked
//! against its type's alignment before it is touched.

use ostd::dma::DmaBuf;

/// Byte-exact little-endian writes into device-visible memory (the DMA area,
/// or a byte vector in host tests).
pub(crate) trait Sink {
    fn put8(&mut self, offset: usize, value: u8);
    fn put16(&mut self, offset: usize, value: u16);
    fn put32(&mut self, offset: usize, value: u32);
    fn put64(&mut self, offset: usize, value: u64);
}

pub(crate) struct DmaArea {
    buf: DmaBuf,
    iova: u64,
}

impl DmaArea {
    /// Allocate `pages` and authorize them for `bdf`; `None` on either failure
    /// (an allocation the device was never authorized for is freed).
    pub(crate) fn new(pages: usize, bdf: u32) -> Option<Self> {
        let buf = DmaBuf::alloc(pages)?;
        match buf.authorize(bdf) {
            Ok(iova) => Some(Self { buf, iova }),
            Err(_) => {
                buf.free();
                None
            }
        }
    }

    /// Device-visible address of byte `offset`.
    pub(crate) fn iova(&self, offset: usize) -> u64 {
        self.iova + offset as u64
    }

    fn at(&self, offset: usize, len: usize) -> *mut u8 {
        assert!(offset
            .checked_add(len)
            .is_some_and(|end| end <= self.buf.size()));
        self.buf.virt().wrapping_add(offset)
    }

    /// The `T` at `offset`: in bounds and aligned for `T`, or a panic, so a
    /// layout bug never reaches device memory as a misaligned access.
    fn field<T>(&self, offset: usize) -> *mut T {
        let field = self.at(offset, core::mem::size_of::<T>()).cast::<T>();
        assert!(field.is_aligned(), "DMA field at {offset:#x} misaligned");
        field
    }

    pub(crate) fn zero(&self, offset: usize, len: usize) {
        // SAFETY: inside this allocation (checked by `at`).
        unsafe { core::ptr::write_bytes(self.at(offset, len), 0, len) };
    }

    pub(crate) fn read8(&self, offset: usize) -> u8 {
        // SAFETY: inside this allocation (checked by `at`).
        unsafe { self.at(offset, 1).read_volatile() }
    }

    pub(crate) fn read32(&self, offset: usize) -> u32 {
        // SAFETY: in bounds and aligned (checked by `field`).
        u32::from_le(unsafe { self.field::<u32>(offset).read_volatile() })
    }

    /// Copy `bytes` to `offset` (frame data, not device-polled).
    pub(crate) fn copy_in(&self, offset: usize, bytes: &[u8]) {
        let dst = self.at(offset, bytes.len());
        // SAFETY: `bytes.len()` bytes at `dst` lie inside this allocation.
        unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len()) };
    }

    /// Copy `out.len()` device-written bytes from `offset`.
    pub(crate) fn copy_out(&self, offset: usize, out: &mut [u8]) {
        let src = self.at(offset, out.len());
        // SAFETY: `out.len()` bytes at `src` lie inside this allocation.
        unsafe { core::ptr::copy_nonoverlapping(src, out.as_mut_ptr(), out.len()) };
    }
}

impl Sink for DmaArea {
    fn put8(&mut self, offset: usize, value: u8) {
        // SAFETY: inside this allocation (checked by `at`).
        unsafe { self.at(offset, 1).write_volatile(value) };
    }

    fn put16(&mut self, offset: usize, value: u16) {
        // SAFETY: in bounds and aligned (checked by `field`).
        unsafe { self.field::<u16>(offset).write_volatile(value.to_le()) };
    }

    fn put32(&mut self, offset: usize, value: u32) {
        // SAFETY: in bounds and aligned (checked by `field`).
        unsafe { self.field::<u32>(offset).write_volatile(value.to_le()) };
    }

    fn put64(&mut self, offset: usize, value: u64) {
        // SAFETY: in bounds and aligned (checked by `field`).
        unsafe { self.field::<u64>(offset).write_volatile(value.to_le()) };
    }
}
