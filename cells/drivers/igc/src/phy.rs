//! Intel I225/I226 PHY access and SW_FW_SYNC synchronization.

use crate::regs::{RegisterFile, MDIC, SWSM, SWSM_SMBI, SW_FW_SYNC};
use types::{ViError, ViResult};

pub(crate) const MDIC_DATA_MASK: u32 = 0x0000_FFFF;
pub(crate) const MDIC_REG_SHIFT: u32 = 16;
pub(crate) const MDIC_PHY_SHIFT: u32 = 21;
pub(crate) const MDIC_OP_WRITE: u32 = 0x0400_0000;
pub(crate) const MDIC_OP_READ: u32 = 0x0800_0000;
pub(crate) const MDIC_READY: u32 = 0x1000_0000;
pub(crate) const MDIC_ERROR: u32 = 0x4000_0000;
pub(crate) const IGC_PHY_ADDR: u32 = 1;

pub(crate) const MII_BMCR: u32 = 0;
pub(crate) const BMCR_SPEED1000: u16 = 1 << 6;
pub(crate) const BMCR_FULL_DUPLEX: u16 = 1 << 8;
pub(crate) const BMCR_ANRESTART: u16 = 1 << 9;
pub(crate) const BMCR_ANENABLE: u16 = 1 << 12;

const MDIC_POLLS: u32 = 100_000;
const SEM_POLLS: u32 = 100_000;

// ── SW / FW Synchronization ──────────────────────────────────────────────────

fn acquire_hw_semaphore(regs: &impl RegisterFile) -> ViResult<()> {
    for _ in 0..SEM_POLLS {
        let swsm = regs.rd32(SWSM)?;
        if swsm & SWSM_SMBI == 0 {
            regs.wr32(SWSM, swsm | SWSM_SMBI)?;
            if regs.rd32(SWSM)? & SWSM_SMBI != 0 {
                return Ok(());
            }
        }
        core::hint::spin_loop();
    }
    Err(ViError::WouldBlock)
}

fn release_hw_semaphore(regs: &impl RegisterFile) -> ViResult<()> {
    let swsm = regs.rd32(SWSM)? & !SWSM_SMBI;
    regs.wr32(SWSM, swsm)
}

pub(crate) fn acquire_swfw_sync(regs: &impl RegisterFile, mask: u32) -> ViResult<()> {
    let fw_mask = mask << 16;
    for _ in 0..SEM_POLLS {
        acquire_hw_semaphore(regs)?;
        let swfw = regs.rd32(SW_FW_SYNC)?;
        if swfw & (mask | fw_mask) == 0 {
            regs.wr32(SW_FW_SYNC, swfw | mask)?;
            let _ = release_hw_semaphore(regs);
            return Ok(());
        }
        let _ = release_hw_semaphore(regs);
        core::hint::spin_loop();
    }
    Err(ViError::WouldBlock)
}

pub(crate) fn release_swfw_sync(regs: &impl RegisterFile, mask: u32) -> ViResult<()> {
    acquire_hw_semaphore(regs)?;
    let swfw = regs.rd32(SW_FW_SYNC)? & !mask;
    let res = regs.wr32(SW_FW_SYNC, swfw);
    let _ = release_hw_semaphore(regs);
    res
}

pub(crate) fn with_swfw_sync<R>(
    regs: &impl RegisterFile,
    mask: u32,
    op: impl FnOnce() -> ViResult<R>,
) -> ViResult<R> {
    acquire_swfw_sync(regs, mask)?;
    let result = op();
    let release_res = release_swfw_sync(regs, mask);
    match (result, release_res) {
        (Ok(val), Ok(())) => Ok(val),
        (Err(e), _) => Err(e),
        (Ok(_), Err(e)) => Err(e),
    }
}

// ── MDIC Access ──────────────────────────────────────────────────────────────

pub(crate) const fn mdic_command(write: bool, reg: u32, data: u16) -> u32 {
    let op = if write { MDIC_OP_WRITE } else { MDIC_OP_READ };
    op | (IGC_PHY_ADDR << MDIC_PHY_SHIFT) | ((reg & 0x1F) << MDIC_REG_SHIFT) | (data as u32)
}

fn mdic_access(regs: &impl RegisterFile, cmd: u32) -> ViResult<u16> {
    regs.wr32(MDIC, cmd)?;
    for _ in 0..MDIC_POLLS {
        let value = regs.rd32(MDIC)?;
        if value & MDIC_READY != 0 {
            if value & MDIC_ERROR != 0 {
                return Err(ViError::IO);
            }
            return Ok((value & MDIC_DATA_MASK) as u16);
        }
        core::hint::spin_loop();
    }
    Err(ViError::IO)
}

pub(crate) fn phy_read(regs: &impl RegisterFile, reg: u32) -> ViResult<u16> {
    mdic_access(regs, mdic_command(false, reg, 0))
}

pub(crate) fn phy_write(regs: &impl RegisterFile, reg: u32, data: u16) -> ViResult<()> {
    mdic_access(regs, mdic_command(true, reg, data)).map(|_| ())
}

pub(crate) fn restart_autoneg(regs: &impl RegisterFile) -> ViResult<()> {
    with_swfw_sync(regs, crate::regs::SWFW_PHY_SM, || {
        let bmcr = phy_read(regs, MII_BMCR)?;
        phy_write(
            regs,
            MII_BMCR,
            bmcr | BMCR_ANENABLE | BMCR_ANRESTART | BMCR_SPEED1000 | BMCR_FULL_DUPLEX,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mdic_command_encoding() {
        let cmd = mdic_command(false, MII_BMCR, 0);
        assert_eq!(cmd & MDIC_OP_READ, MDIC_OP_READ);
        assert_eq!((cmd >> MDIC_PHY_SHIFT) & 0x1F, IGC_PHY_ADDR);
    }
}
