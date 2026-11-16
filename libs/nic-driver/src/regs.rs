//! The register-file seam: chip code reads and writes registers through
//! these traits, so host tests drive it with a fake instead of a BAR.

use ostd::mmio::MmioRegion;
use types::ViResult;

/// 32-bit device registers.
pub trait RegisterFile {
    fn rd32(&self, offset: usize) -> ViResult<u32>;
    fn wr32(&self, offset: usize, value: u32) -> ViResult<()>;
}

/// 8- and 16-bit device registers, for chips that have them (Realtek).
pub trait NarrowRegisterFile: RegisterFile {
    fn rd8(&self, offset: usize) -> ViResult<u8>;
    fn wr8(&self, offset: usize, value: u8) -> ViResult<()>;
    fn rd16(&self, offset: usize) -> ViResult<u16>;
    fn wr16(&self, offset: usize, value: u16) -> ViResult<()>;
}

impl RegisterFile for MmioRegion {
    #[inline]
    fn rd32(&self, offset: usize) -> ViResult<u32> {
        self.read_u32(offset)
    }

    #[inline]
    fn wr32(&self, offset: usize, value: u32) -> ViResult<()> {
        self.write_u32(offset, value)
    }
}

impl NarrowRegisterFile for MmioRegion {
    #[inline]
    fn rd8(&self, offset: usize) -> ViResult<u8> {
        self.read_u8(offset)
    }

    #[inline]
    fn wr8(&self, offset: usize, value: u8) -> ViResult<()> {
        self.write_u8(offset, value)
    }

    #[inline]
    fn rd16(&self, offset: usize) -> ViResult<u16> {
        self.read::<u16>(offset)
    }

    #[inline]
    fn wr16(&self, offset: usize, value: u16) -> ViResult<()> {
        self.write::<u16>(offset, value)
    }
}
