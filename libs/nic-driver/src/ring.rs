//! Descriptor rings and frame buffers in device-visible DMA pages.
//!
//! Law 4 exception: the volatile accesses and copies below touch memory the
//! controller reads and writes. Every ring and buffer is a whole `DmaBuf` the
//! cell owns for its lifetime (mapped read-write at `virt()` for `size()`
//! bytes, page-aligned), and every access is checked against that size before
//! it is made. Nothing here programs a device address: that stays behind
//! [`crate::dma::DmaSet::alloc_authorized`].

use core::marker::PhantomData;
use core::mem::{align_of, size_of};
use ostd::dma::DmaBuf;
use types::{ViError, ViResult};

const PAGE: usize = 4096;

/// Zero a whole DMA buffer.
pub fn zero(buf: &DmaBuf) {
    // SAFETY: `buf` is an owned, writable DMA mapping of `buf.size()` bytes.
    unsafe { core::ptr::write_bytes(buf.virt(), 0, buf.size()) };
}

/// `N` descriptors of type `D` at the start of one DMA buffer.
pub struct DescRing<D: Copy, const N: usize> {
    buf: DmaBuf,
    _desc: PhantomData<D>,
}

impl<D: Copy, const N: usize> DescRing<D, N> {
    /// Wrap `buf`; `InvalidArgument` when `N` descriptors do not fit it.
    pub fn new(buf: DmaBuf) -> ViResult<Self> {
        if !fits::<D>(N, buf.size()) {
            return Err(ViError::InvalidArgument);
        }
        Ok(Self {
            buf,
            _desc: PhantomData,
        })
    }

    /// The backing buffer (to authorize it).
    pub fn dma(&self) -> &DmaBuf {
        &self.buf
    }

    /// Zero the whole ring page.
    pub fn clear(&self) {
        zero(&self.buf);
    }

    /// Volatile read of descriptor `index` (< `N`, else panics).
    pub fn read(&self, index: usize) -> D {
        // SAFETY: `slot` bounds `index` to the `N` descriptors `new` proved
        // fit the owned buffer, at a `D`-aligned offset of a page-aligned base.
        unsafe { self.slot(index).read_volatile() }
    }

    /// Volatile write of descriptor `index` (< `N`, else panics).
    pub fn write(&self, index: usize, desc: D) {
        // SAFETY: as in `read`; the mapping is writable.
        unsafe { self.slot(index).write_volatile(desc) }
    }

    fn slot(&self, index: usize) -> *mut D {
        assert!(
            index < N,
            "descriptor index {index} outside a {N}-slot ring"
        );
        (self.buf.virt() as *mut D).wrapping_add(index)
    }
}

/// Whether `n` descriptors of `D` fit `bytes` of a page-aligned buffer.
const fn fits<D>(n: usize, bytes: usize) -> bool {
    align_of::<D>() <= PAGE
        && match n.checked_mul(size_of::<D>()) {
            Some(len) => len <= bytes,
            None => false,
        }
}

/// `N` one-page frame buffers.
pub struct FrameBufs<const N: usize> {
    bufs: [DmaBuf; N],
}

impl<const N: usize> FrameBufs<N> {
    /// Allocate `N` single-page buffers.
    pub fn alloc() -> ViResult<Self> {
        let bufs = crate::dma::try_init_array(|_| DmaBuf::alloc(1).ok_or(ViError::OutOfMemory))?;
        Ok(Self { bufs })
    }

    /// Buffer `index` (to authorize it).
    pub fn dma(&self, index: usize) -> &DmaBuf {
        &self.bufs[index]
    }

    /// Copy `frame` into buffer `index`; `InvalidArgument` when it does not
    /// fit, and nothing is copied.
    pub fn copy_in(&self, index: usize, frame: &[u8]) -> ViResult<()> {
        let buf = &self.bufs[index];
        if frame.len() > buf.size() {
            return Err(ViError::InvalidArgument);
        }
        // SAFETY: `frame.len()` bytes fit the owned, writable buffer (checked
        // above), and a cell slice never aliases a DMA page.
        unsafe { core::ptr::copy_nonoverlapping(frame.as_ptr(), buf.virt(), frame.len()) };
        Ok(())
    }

    /// Copy up to `len` received bytes of buffer `index` into `out`; returns
    /// the count copied, bounded by `len`, `out` and the buffer.
    pub fn copy_out(&self, index: usize, out: &mut [u8], len: usize) -> usize {
        let buf = &self.bufs[index];
        let n = copy_len(len, out.len(), buf.size());
        // SAFETY: `n` bytes lie inside both the owned buffer and `out`.
        unsafe { core::ptr::copy_nonoverlapping(buf.virt(), out.as_mut_ptr(), n) };
        n
    }
}

fn copy_len(len: usize, out: usize, buf: usize) -> usize {
    len.min(out).min(buf)
}

#[cfg(test)]
mod tests {
    use super::{copy_len, fits};

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Desc16(u64, u64);

    #[test]
    fn ring_capacity_is_checked_against_the_buffer() {
        assert!(fits::<Desc16>(16, 4096));
        assert!(fits::<Desc16>(256, 4096));
        assert!(!fits::<Desc16>(257, 4096));
        assert!(!fits::<Desc16>(usize::MAX, 4096), "overflow refuses");
    }

    #[test]
    fn frame_copy_is_bounded_by_length_destination_and_buffer() {
        assert_eq!(copy_len(60, 2048, 4096), 60);
        assert_eq!(copy_len(3000, 2048, 4096), 2048, "destination");
        assert_eq!(
            copy_len(0xFFFF, 0x1_0000, 4096),
            4096,
            "device-written length"
        );
    }
}
