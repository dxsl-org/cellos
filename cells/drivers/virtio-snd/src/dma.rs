//! The cell's one DMA allocation, as [`Mem`].

use crate::ring::DMA_PAGES;
use crate::virtq::Mem;
use core::fmt;
use ostd::dma::DmaBuf;
use ostd::syscall::SyscallError;

/// [`DMA_PAGES`] pages authorized for one device, laid out per
/// [`crate::ring`]. Never freed: the device may hold addresses in it until
/// it is reset, and the cell resets it only on exit.
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
}

impl Mem for DmaArea {
    fn write(&self, off: usize, bytes: &[u8]) {
        assert!(self.buf.write_bytes(off, bytes), "DMA write out of range");
    }

    fn read(&self, off: usize, out: &mut [u8]) {
        assert!(self.buf.read_bytes(off, out), "DMA read out of range");
    }

    fn iova(&self, off: usize) -> u64 {
        self.iova + off as u64
    }
}
