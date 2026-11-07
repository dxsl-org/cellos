//! Realtek RTL8168/8111 and RTL8125 register definitions.

#![allow(dead_code)]

use ostd::mmio::MmioRegion;
use types::ViResult;

pub(crate) const IDR0: usize = 0x0000;
pub(crate) const TXLIST_ADDR_LO: usize = 0x0020;
pub(crate) const TXLIST_ADDR_HI: usize = 0x0024;
pub(crate) const COMMAND: usize = 0x0037;
pub(crate) const TPPOLL: usize = 0x0038;
pub(crate) const IMR: usize = 0x003C;
pub(crate) const ISR: usize = 0x003E;
pub(crate) const TXCFG: usize = 0x0040;
pub(crate) const RXCFG: usize = 0x0044;
pub(crate) const PHYAR: usize = 0x0060;
pub(crate) const CPLUS_CMD: usize = 0x00E0;
pub(crate) const RXLIST_ADDR_LO: usize = 0x00E4;
pub(crate) const RXLIST_ADDR_HI: usize = 0x00E8;

// ── Bits ─────────────────────────────────────────────────────────────────────

pub(crate) const CR_RST: u8 = 0x10;
pub(crate) const CR_RE: u8 = 0x08;
pub(crate) const CR_TE: u8 = 0x04;

pub(crate) const TPPOLL_NPQ: u8 = 0x01;
pub(crate) const TPPOLL_HPQ: u8 = 0x40;

pub(crate) const CPLUSCMD_TXENB: u16 = 0x0001;
pub(crate) const CPLUSCMD_RXENB: u16 = 0x0002;
pub(crate) const CPLUSCMD_PCI_MULRW: u16 = 0x0008;

pub(crate) const TXCFG_IFG: u32 = 0x0300_0000;
pub(crate) const TXCFG_MAXDMA: u32 = 0x0000_0700;
pub(crate) const RXCFG_MAXDMA: u32 = 0x0000_0700;
pub(crate) const RXCFG_ACCEPT_ALL: u32 = 0x0000_000F; // Broacast + Multicast + MyPhys + AllPhys

pub(crate) const PHYAR_BUSY: u32 = 0x8000_0000;
pub(crate) const PHYAR_REG_SHIFT: u32 = 16;
pub(crate) const PHYAR_DATA_MASK: u32 = 0x0000_FFFF;

pub(crate) trait RegisterFile {
    fn rd8(&self, offset: usize) -> ViResult<u8>;
    fn wr8(&self, offset: usize, value: u8) -> ViResult<()>;
    fn rd16(&self, offset: usize) -> ViResult<u16>;
    fn wr16(&self, offset: usize, value: u16) -> ViResult<()>;
    fn rd32(&self, offset: usize) -> ViResult<u32>;
    fn wr32(&self, offset: usize, value: u32) -> ViResult<()>;
}

impl RegisterFile for MmioRegion {
    fn rd8(&self, offset: usize) -> ViResult<u8> {
        self.read_u8(offset)
    }

    fn wr8(&self, offset: usize, value: u8) -> ViResult<()> {
        self.write_u8(offset, value)
    }

    fn rd16(&self, offset: usize) -> ViResult<u16> {
        self.read::<u16>(offset)
    }

    fn wr16(&self, offset: usize, value: u16) -> ViResult<()> {
        self.write::<u16>(offset, value)
    }

    fn rd32(&self, offset: usize) -> ViResult<u32> {
        self.read_u32(offset)
    }

    fn wr32(&self, offset: usize, value: u32) -> ViResult<()> {
        self.write_u32(offset, value)
    }
}
