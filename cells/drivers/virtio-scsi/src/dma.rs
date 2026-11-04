//! Volatile access to the queue's DMA allocation.

use crate::ring::DMA_PAGES;
use core::fmt;
use ostd::dma::DmaBuf;
use ostd::syscall::SyscallError;

/// [`DMA_PAGES`] page-aligned pages authorized for one device, laid out per
/// [`crate::ring`]. Never freed: the device may hold the address until reset.
///
/// Accessors take offsets from the region map `crate::ring` proves to lie
/// inside the allocation, and multi-byte fields at their natural alignment;
/// an offset past the end panics instead of touching foreign memory.
pub struct DmaArea {
    buf: DmaBuf,
    iova: u64,
}

/// Why the DMA area could not be set up.
#[derive(Debug)]
pub enum DmaError {
    Alloc,
    Authorize(SyscallError),
}

impl fmt::Display for DmaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Alloc => write!(f, "DMA allocation of {DMA_PAGES} pages failed"),
            Self::Authorize(error) => write!(f, "DMA authorization failed: {error:?}"),
        }
    }
}

impl DmaArea {
    /// Allocates the area and authorizes it for `bdf`, keeping the returned
    /// device-visible address. A buffer the device was never authorized for
    /// is returned to the kernel on failure.
    pub fn new(bdf: u32) -> Result<Self, DmaError> {
        let buf = DmaBuf::alloc(DMA_PAGES).ok_or(DmaError::Alloc)?;
        match buf.authorize(bdf) {
            Ok(iova) => Ok(Self { buf, iova }),
            Err(error) => {
                buf.free();
                Err(DmaError::Authorize(error))
            }
        }
    }

    /// Device-visible address of byte `off`.
    pub fn iova(&self, off: usize) -> u64 {
        self.iova + off as u64
    }

    fn at(&self, off: usize, len: usize) -> *mut u8 {
        assert!(off
            .checked_add(len)
            .is_some_and(|end| end <= self.buf.size()));
        self.buf.virt().wrapping_add(off)
    }

    pub fn write_bytes(&self, off: usize, bytes: &[u8]) {
        let base = self.at(off, bytes.len());
        for (i, &byte) in bytes.iter().enumerate() {
            // SAFETY: inside this allocation (checked by `at`).
            unsafe { base.add(i).write_volatile(byte) };
        }
    }

    pub fn fill_zero(&self, off: usize, len: usize) {
        let base = self.at(off, len);
        for i in 0..len {
            // SAFETY: inside this allocation (checked by `at`).
            unsafe { base.add(i).write_volatile(0) };
        }
    }

    /// Volatile: the device may have written these bytes.
    pub fn read_bytes(&self, off: usize, out: &mut [u8]) {
        let base = self.at(off, out.len());
        for (i, byte) in out.iter_mut().enumerate() {
            // SAFETY: inside this allocation (checked by `at`).
            *byte = unsafe { base.add(i).read_volatile() };
        }
    }

    pub fn write_u16(&self, off: usize, value: u16) {
        // SAFETY: in bounds (checked by `at`); callers pass 2-aligned fields.
        unsafe { self.at(off, 2).cast::<u16>().write_volatile(value.to_le()) };
    }

    pub fn write_u32(&self, off: usize, value: u32) {
        // SAFETY: in bounds (checked by `at`); callers pass 4-aligned fields.
        unsafe { self.at(off, 4).cast::<u32>().write_volatile(value.to_le()) };
    }

    pub fn write_u64(&self, off: usize, value: u64) {
        // SAFETY: in bounds (checked by `at`); callers pass 8-aligned fields.
        unsafe { self.at(off, 8).cast::<u64>().write_volatile(value.to_le()) };
    }

    pub fn read_u16(&self, off: usize) -> u16 {
        // SAFETY: in bounds (checked by `at`); callers pass 2-aligned fields.
        u16::from_le(unsafe { self.at(off, 2).cast::<u16>().read_volatile() })
    }

    pub fn read_u32(&self, off: usize) -> u32 {
        // SAFETY: in bounds (checked by `at`); callers pass 4-aligned fields.
        u32::from_le(unsafe { self.at(off, 4).cast::<u32>().read_volatile() })
    }
}
