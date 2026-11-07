//! Broadcom BCM57xx MII PHY communication under APE PHY lock.

use crate::ape::{self, with_ape_lock};
use crate::regs::{
    RegisterFile, MICOMM_BUSY, MICOMM_CMD_READ, MICOMM_CMD_WRITE, MICOMM_DATA_MASK,
    MICOMM_PHY_SHIFT, MICOMM_READFAIL, MICOMM_REG_SHIFT, MI_COMM,
};
use types::{ViError, ViResult};

pub(crate) const MII_BMCR: u32 = 0;
pub(crate) const BMCR_SPEED1000: u16 = 1 << 6;
pub(crate) const BMCR_FULL_DUPLEX: u16 = 1 << 8;
pub(crate) const BMCR_ANRESTART: u16 = 1 << 9;
pub(crate) const BMCR_ANENABLE: u16 = 1 << 12;

pub(crate) const BCM57XX_PHY_ADDR: u32 = 1;

const MI_POLLS: u32 = 50_000;

fn mi_access(regs: &impl RegisterFile, cmd: u32) -> ViResult<u16> {
    regs.wr32(MI_COMM, cmd | MICOMM_BUSY)?;
    for _ in 0..MI_POLLS {
        let val = regs.rd32(MI_COMM)?;
        if val & MICOMM_BUSY == 0 {
            if val & MICOMM_READFAIL != 0 {
                return Err(ViError::IO);
            }
            return Ok((val & MICOMM_DATA_MASK) as u16);
        }
        core::hint::spin_loop();
    }
    Err(ViError::IO)
}

pub(crate) fn phy_read(regs: &impl RegisterFile, func: u8, reg: u32) -> ViResult<u16> {
    let locknum = ape::phy_lock_for_func(func);
    with_ape_lock(regs, func, locknum, || {
        let cmd = MICOMM_CMD_READ
            | (BCM57XX_PHY_ADDR << MICOMM_PHY_SHIFT)
            | ((reg & 0x1F) << MICOMM_REG_SHIFT);
        mi_access(regs, cmd)
    })
}

pub(crate) fn phy_write(regs: &impl RegisterFile, func: u8, reg: u32, data: u16) -> ViResult<()> {
    let locknum = ape::phy_lock_for_func(func);
    with_ape_lock(regs, func, locknum, || {
        let cmd = MICOMM_CMD_WRITE
            | (BCM57XX_PHY_ADDR << MICOMM_PHY_SHIFT)
            | ((reg & 0x1F) << MICOMM_REG_SHIFT)
            | (data as u32 & MICOMM_DATA_MASK);
        mi_access(regs, cmd).map(|_| ())
    })
}

pub(crate) fn restart_autoneg(regs: &impl RegisterFile, func: u8) -> ViResult<()> {
    let bmcr = phy_read(regs, func, MII_BMCR)?;
    phy_write(
        regs,
        func,
        MII_BMCR,
        bmcr | BMCR_ANENABLE | BMCR_ANRESTART | BMCR_SPEED1000 | BMCR_FULL_DUPLEX,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phy_addr_is_one() {
        assert_eq!(BCM57XX_PHY_ADDR, 1);
    }
}
