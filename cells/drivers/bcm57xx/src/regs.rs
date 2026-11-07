//! Broadcom NetXtreme BCM57xx register map and MMIO trait.

#![allow(dead_code)]

use ostd::mmio::MmioRegion;
use types::ViResult;

pub(crate) const MAC_MODE: usize = 0x0400;
pub(crate) const MAC_STATUS: usize = 0x0404;
pub(crate) const MAC_ADDR1_LO: usize = 0x0410;
pub(crate) const MAC_ADDR1_HI: usize = 0x0414;
pub(crate) const MI_COMM: usize = 0x044C;
pub(crate) const MI_MODE: usize = 0x0454;

pub(crate) const RX_STD_PRODIDX_REG: usize = 0x026C;
pub(crate) const TX_PRODIDX_REG: usize = 0x0304;

pub(crate) const MODE_CTL: usize = 0x6800;
pub(crate) const MISC_CFG: usize = 0x6804;

// ── Bits ─────────────────────────────────────────────────────────────────────

pub(crate) const MISCCFG_RESET_CORE_CLOCKS: u32 = 0x0000_0001;
pub(crate) const MACMODE_PORTMODE_GMII: u32 = 0x0000_0002;
pub(crate) const MACMODE_LINK_POLARITY: u32 = 0x0000_0400;

pub(crate) const MICOMM_DATA_MASK: u32 = 0x0000_FFFF;
pub(crate) const MICOMM_REG_SHIFT: u32 = 16;
pub(crate) const MICOMM_PHY_SHIFT: u32 = 21;
pub(crate) const MICOMM_CMD_WRITE: u32 = 0x0400_0000;
pub(crate) const MICOMM_CMD_READ: u32 = 0x0800_0000;
pub(crate) const MICOMM_BUSY: u32 = 0x2000_0000;
pub(crate) const MICOMM_READFAIL: u32 = 0x1000_0000;

pub(crate) trait RegisterFile {
    fn rd32(&self, offset: usize) -> ViResult<u32>;
    fn wr32(&self, offset: usize, value: u32) -> ViResult<()>;
}

impl RegisterFile for MmioRegion {
    fn rd32(&self, offset: usize) -> ViResult<u32> {
        self.read_u32(offset)
    }

    fn wr32(&self, offset: usize, value: u32) -> ViResult<()> {
        self.write_u32(offset, value)
    }
}
