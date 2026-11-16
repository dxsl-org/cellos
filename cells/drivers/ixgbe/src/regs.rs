//! Intel 82599/X540/X550 register definitions and MMIO trait.

#![allow(dead_code)]

use types::{ViError, ViResult};

pub(crate) const CTRL: usize = 0x0_0000;
pub(crate) const STATUS: usize = 0x0_0008;
pub(crate) const CTRL_EXT: usize = 0x0_0018;
pub(crate) const I2CCTL_82599: usize = 0x0_0028;

pub(crate) const RXCTRL: usize = 0x0_3000;
pub(crate) const DMATXCTL: usize = 0x0_4A80;
pub(crate) const FCTRL: usize = 0x0_5080;
pub(crate) const RAL0: usize = 0x0_5400;
pub(crate) const RAH0: usize = 0x0_5404;

// Queue 0 registers
pub(crate) const RDBAL0: usize = 0x0_1000;
pub(crate) const RDBAH0: usize = 0x0_1004;
pub(crate) const RDLEN0: usize = 0x0_1008;
pub(crate) const RDH0: usize = 0x0_1010;
pub(crate) const RDT0: usize = 0x0_1018;
pub(crate) const RXDCTL0: usize = 0x0_1028;

pub(crate) const TDBAL0: usize = 0x0_6000;
pub(crate) const TDBAH0: usize = 0x0_6004;
pub(crate) const TDLEN0: usize = 0x0_6008;
pub(crate) const TDH0: usize = 0x0_6010;
pub(crate) const TDT0: usize = 0x0_6018;
pub(crate) const TXDCTL0: usize = 0x0_6028;

pub(crate) const EERD: usize = 0x1_0014;

// ── Bits ─────────────────────────────────────────────────────────────────────

pub(crate) const CTRL_RST: u32 = 0x0400_0000;
pub(crate) const CTRL_GIO_DIS: u32 = 0x0000_0004;
pub(crate) const STATUS_LU: u32 = 0x0000_0002;

pub(crate) const RXCTRL_RXEN: u32 = 0x0000_0001;
pub(crate) const DMATXCTL_TE: u32 = 0x0000_0001;

pub(crate) const FCTRL_BAM: u32 = 0x0000_0400;
pub(crate) const FCTRL_MPE: u32 = 0x0000_0100;
pub(crate) const FCTRL_UPE: u32 = 0x0000_0200;

pub(crate) const RAH_AV: u32 = 0x8000_0000;

pub(crate) const RXDCTL_ENABLE: u32 = 0x0200_0000;
pub(crate) const TXDCTL_ENABLE: u32 = 0x0200_0000;

pub(crate) const EERD_START: u32 = 0x0000_0001;
pub(crate) const EERD_DONE: u32 = 0x0000_0002;
pub(crate) const EERD_ADDR_SHIFT: u32 = 2;
pub(crate) const EERD_DATA_SHIFT: u32 = 16;

pub(crate) const I2C_CLK_IN: u32 = 0x0000_0001;
pub(crate) const I2C_CLK_OUT: u32 = 0x0000_0002;
pub(crate) const I2C_DATA_IN: u32 = 0x0000_0004;
pub(crate) const I2C_DATA_OUT: u32 = 0x0000_0008;

const NVM_POLLS: u32 = 100_000;

pub(crate) use nic_driver::regs::RegisterFile;

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
        let cmd = ((5u32) << EERD_ADDR_SHIFT) | EERD_START;
        assert_eq!(cmd & EERD_START, EERD_START);
        assert_eq!((cmd >> EERD_ADDR_SHIFT) & 0x3FFF, 5);
    }
}
