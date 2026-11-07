#![allow(dead_code)]

use ostd::mmio::MmioRegion;
use types::{ViError, ViResult};

pub(crate) const CTRL: usize = 0x0_0000;
pub(crate) const STATUS: usize = 0x0_0008;
pub(crate) const CTRL_EXT: usize = 0x0_0018;
pub(crate) const MDIC: usize = 0x0_0020;
pub(crate) const RCTL: usize = 0x0_0100;
pub(crate) const TCTL: usize = 0x0_0400;
pub(crate) const TIPG: usize = 0x0_0410;
pub(crate) const EEER: usize = 0x0_0E30;
pub(crate) const EEE_SU: usize = 0x0_0E34;
pub(crate) const ICR: usize = 0x0_1500;
pub(crate) const IMS: usize = 0x0_1508;
pub(crate) const IMC: usize = 0x0_150C;
pub(crate) const MTA: usize = 0x0_5200;
pub(crate) const MTA_ENTRIES: usize = 128;
pub(crate) const RAL0: usize = 0x0_5400;
pub(crate) const RAH0: usize = 0x0_5404;
pub(crate) const SWSM: usize = 0x0_5B50;
pub(crate) const FWSM: usize = 0x0_5B54;
pub(crate) const SW_FW_SYNC: usize = 0x0_5B5C;

// Queue 0 registers
pub(crate) const RDBAL0: usize = 0x0_C000;
pub(crate) const RDBAH0: usize = 0x0_C004;
pub(crate) const RDLEN0: usize = 0x0_C008;
pub(crate) const SRRCTL0: usize = 0x0_C00C;
pub(crate) const RDH0: usize = 0x0_C010;
pub(crate) const RDT0: usize = 0x0_C018;
pub(crate) const RXDCTL0: usize = 0x0_C028;
pub(crate) const TDBAL0: usize = 0x0_E000;
pub(crate) const TDBAH0: usize = 0x0_E004;
pub(crate) const TDLEN0: usize = 0x0_E008;
pub(crate) const TDH0: usize = 0x0_E010;
pub(crate) const TDT0: usize = 0x0_E018;
pub(crate) const TXDCTL0: usize = 0x0_E028;

// I225/I226 EERD is at 0x12014
pub(crate) const EERD: usize = 0x1_2014;

// ── Bits ─────────────────────────────────────────────────────────────────────

pub(crate) const CTRL_RST: u32 = 1 << 26;
pub(crate) const CTRL_DEV_RST: u32 = 1 << 29;
pub(crate) const CTRL_SLU: u32 = 1 << 6;
pub(crate) const CTRL_ASDE: u32 = 1 << 5;
pub(crate) const STATUS_LU: u32 = 1 << 1;

pub(crate) const RCTL_EN: u32 = 1 << 1;
pub(crate) const RCTL_UPE: u32 = 1 << 3;
pub(crate) const RCTL_MPE: u32 = 1 << 4;
pub(crate) const RCTL_BAM: u32 = 1 << 15;
pub(crate) const RCTL_SECRC: u32 = 1 << 26;

pub(crate) const TCTL_EN: u32 = 1 << 1;
pub(crate) const TCTL_PSP: u32 = 1 << 3;
pub(crate) const TCTL_CT: u32 = 0x0F << 4;
pub(crate) const TCTL_COLD: u32 = 0x40 << 12;
pub(crate) const TIPG_COPPER: u32 = 0x0060_200A;

pub(crate) const RAH_AV: u32 = 1 << 31;
pub(crate) const RXDCTL_ENABLE: u32 = 1 << 25;
pub(crate) const TXDCTL_ENABLE: u32 = 1 << 25;
pub(crate) const SRRCTL_DESCTYPE_ADV_ONEBUF: u32 = 0x0200_0000;

pub(crate) const EERD_START: u32 = 1 << 0;
pub(crate) const EERD_DONE: u32 = 1 << 1;
pub(crate) const EERD_ADDR_SHIFT: u32 = 2;
pub(crate) const EERD_DATA_SHIFT: u32 = 16;

pub(crate) const SWSM_SMBI: u32 = 1 << 0;
pub(crate) const SWSM_SWESMBI: u32 = 1 << 1;
pub(crate) const SWFW_PHY_SM: u32 = 1 << 1;
pub(crate) const SWFW_EEP_SM: u32 = 1 << 0;

const NVM_POLLS: u32 = 100_000;

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

pub(crate) fn nvm_read(regs: &impl RegisterFile, word: u16) -> ViResult<u16> {
    let cmd = ((word as u32) << EERD_ADDR_SHIFT) | EERD_START;
    regs.wr32(EERD, cmd)?;
    for _ in 0..NVM_POLLS {
        let value = regs.rd32(EERD)?;
        if value & EERD_DONE != 0 {
            return Ok((value >> EERD_DATA_SHIFT) as u16);
        }
        core::hint::spin_loop();
    }
    Err(ViError::IO)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eerd_command_encoding() {
        let cmd = ((3u32) << EERD_ADDR_SHIFT) | EERD_START;
        assert_eq!(cmd & EERD_START, EERD_START);
        assert_eq!((cmd >> EERD_ADDR_SHIFT) & 0x3FFF, 3);
    }
}
