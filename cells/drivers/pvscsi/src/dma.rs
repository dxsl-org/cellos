//! The adapter's one DMA allocation: ring state, request ring, completion
//! ring, sense buffer and data bounce buffer, laid out by [`crate::rings`].
//!
//! Law 4 exception: the volatile accesses below touch device-visible memory.
//! The allocation is owned for the cell's lifetime (the device may hold its
//! addresses until reset), it is authorized for the adapter's BDF before any
//! of its addresses is programmed, and every access is bounds-checked against
//! its length; 32-bit fields are naturally aligned by the layout.

use ostd::dma::DmaBuf;

pub struct DmaArea {
    buf: DmaBuf,
    iova: u64,
}

impl DmaArea {
    /// Allocate `pages` and authorize them for `bdf`; `None` on either failure
    /// (an allocation the device was never authorized for is freed).
    pub fn new(pages: usize, bdf: u32) -> Option<Self> {
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
    pub fn iova(&self, offset: usize) -> u64 {
        self.iova + offset as u64
    }

    fn at(&self, offset: usize, len: usize) -> *mut u8 {
        assert!(offset
            .checked_add(len)
            .is_some_and(|end| end <= self.buf.size()));
        self.buf.virt().wrapping_add(offset)
    }

    pub fn zero(&self, offset: usize, len: usize) {
        let base = self.at(offset, len);
        for i in 0..len {
            // SAFETY: inside this allocation (checked by `at`).
            unsafe { base.add(i).write_volatile(0) };
        }
    }

    pub fn read32(&self, offset: usize) -> u32 {
        // SAFETY: in bounds (checked by `at`); the layout aligns 4-byte fields.
        u32::from_le(unsafe { self.at(offset, 4).cast::<u32>().read_volatile() })
    }

    pub fn write32(&self, offset: usize, value: u32) {
        // SAFETY: in bounds (checked by `at`); the layout aligns 4-byte fields.
        unsafe {
            self.at(offset, 4)
                .cast::<u32>()
                .write_volatile(value.to_le())
        };
    }

    /// Volatile byte copy into the area.
    pub fn copy_in(&self, offset: usize, bytes: &[u8]) {
        let base = self.at(offset, bytes.len());
        for (i, &byte) in bytes.iter().enumerate() {
            // SAFETY: inside this allocation (checked by `at`).
            unsafe { base.add(i).write_volatile(byte) };
        }
    }

    /// Volatile byte copy out of the area: the device may have written it.
    pub fn copy_out(&self, offset: usize, out: &mut [u8]) {
        let base = self.at(offset, out.len());
        for (i, byte) in out.iter_mut().enumerate() {
            // SAFETY: inside this allocation (checked by `at`).
            *byte = unsafe { base.add(i).read_volatile() };
        }
    }
}
