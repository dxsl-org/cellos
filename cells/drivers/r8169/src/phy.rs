//! Realtek RTL8168/RTL8125 MII PHY access via PHYAR.

use crate::regs::{RegisterFile, PHYAR, PHYAR_BUSY, PHYAR_DATA_MASK, PHYAR_REG_SHIFT};
use types::{ViError, ViResult};

pub(crate) const MII_BMCR: u32 = 0;
pub(crate) const BMCR_SPEED1000: u16 = 1 << 6;
pub(crate) const BMCR_FULL_DUPLEX: u16 = 1 << 8;
pub(crate) const BMCR_ANRESTART: u16 = 1 << 9;
pub(crate) const BMCR_ANENABLE: u16 = 1 << 12;

const PHY_POLLS: u32 = 100_000;

pub(crate) fn phy_read(regs: &impl RegisterFile, reg: u32) -> ViResult<u16> {
    let cmd = (reg & 0x1F) << PHYAR_REG_SHIFT;
    regs.wr32(PHYAR, cmd)?;

    for _ in 0..PHY_POLLS {
        let val = regs.rd32(PHYAR)?;
        if val & PHYAR_BUSY != 0 {
            return Ok((val & PHYAR_DATA_MASK) as u16);
        }
        core::hint::spin_loop();
    }
    Err(ViError::IO)
}

pub(crate) fn phy_write(regs: &impl RegisterFile, reg: u32, data: u16) -> ViResult<()> {
    let cmd = PHYAR_BUSY | ((reg & 0x1F) << PHYAR_REG_SHIFT) | (data as u32 & PHYAR_DATA_MASK);
    regs.wr32(PHYAR, cmd)?;

    for _ in 0..PHY_POLLS {
        let val = regs.rd32(PHYAR)?;
        if val & PHYAR_BUSY == 0 {
            return Ok(());
        }
        core::hint::spin_loop();
    }
    Err(ViError::IO)
}

pub(crate) fn restart_autoneg(regs: &impl RegisterFile) -> ViResult<()> {
    let bmcr = phy_read(regs, MII_BMCR)?;
    phy_write(
        regs,
        MII_BMCR,
        bmcr | BMCR_ANENABLE | BMCR_ANRESTART | BMCR_SPEED1000 | BMCR_FULL_DUPLEX,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phy_command_masks() {
        let reg = 16u32;
        let cmd = (reg & 0x1F) << PHYAR_REG_SHIFT;
        assert_eq!((cmd >> PHYAR_REG_SHIFT) & 0x1F, reg);
    }
}
