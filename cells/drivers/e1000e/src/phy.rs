//! 82574 PHY access through `MDIC` and copper autonegotiation.
//!
//! `MDIC` layout (82574 datasheet §10.2.2.9): data 15:0, PHY register 20:16,
//! PHY address 25:21, opcode 27:26 (`01` write, `10` read), ready 28,
//! interrupt-enable 29, error 30. The integrated PHY answers at address 1.
//! MII register numbers and bits are IEEE 802.3 clause 22.
//!
//! After a global reset the PHY restarts from its power-on defaults and
//! `STATUS.LU` follows the PHY's autonegotiation, so the cell restarts autoneg
//! explicitly and then waits for link (bounded, see `controller.rs`).

use crate::identity::MacKind;
use crate::regs::{RegisterFile, MDIC};
use types::{ViError, ViResult};

pub(crate) const MDIC_DATA_MASK: u32 = 0x0000_FFFF;
pub(crate) const MDIC_REG_SHIFT: u32 = 16;
pub(crate) const MDIC_PHY_SHIFT: u32 = 21;
pub(crate) const MDIC_OP_WRITE: u32 = 0b01 << 26;
pub(crate) const MDIC_OP_READ: u32 = 0b10 << 26;
pub(crate) const MDIC_READY: u32 = 1 << 28;
pub(crate) const MDIC_ERROR: u32 = 1 << 30;
/// The 82574's integrated PHY address.
pub(crate) const PHY_ADDR: u32 = 1;

/// MII Basic Mode Control Register.
pub(crate) const MII_BMCR: u32 = 0;
pub(crate) const BMCR_SPEED1000: u16 = 1 << 6;
pub(crate) const BMCR_FULL_DUPLEX: u16 = 1 << 8;
pub(crate) const BMCR_ANRESTART: u16 = 1 << 9;
pub(crate) const BMCR_ANENABLE: u16 = 1 << 12;

/// `MDIC` ready poll budget.
const MDIC_POLLS: u32 = 100_000;

/// The `MDIC` command word for one clause-22 access to the integrated PHY.
pub(crate) const fn mdic_command(write: bool, reg: u32, data: u16) -> u32 {
    let op = if write { MDIC_OP_WRITE } else { MDIC_OP_READ };
    (data as u32 & MDIC_DATA_MASK) | (reg << MDIC_REG_SHIFT) | (PHY_ADDR << MDIC_PHY_SHIFT) | op
}

/// Issue one `MDIC` access and wait (bounded) for `READY`; `ERROR` fails.
fn mdic_access(regs: &impl RegisterFile, command: u32) -> ViResult<u16> {
    regs.wr32(MDIC, command)?;
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

/// Advertise autonegotiation and restart it. The link then comes up when the
/// PHY completes autoneg; the caller waits on `STATUS.LU`.
pub(crate) fn restart_autoneg(regs: &impl RegisterFile, kind: MacKind) -> ViResult<()> {
    match kind {
        MacKind::I82574 => {
            let bmcr = phy_read(regs, MII_BMCR)?;
            phy_write(
                regs,
                MII_BMCR,
                bmcr | BMCR_ANENABLE | BMCR_ANRESTART | BMCR_SPEED1000 | BMCR_FULL_DUPLEX,
            )
        }
        MacKind::PchLan(_) => crate::pch::with_swflag(regs, || {
            let bmcr = phy_read(regs, MII_BMCR)?;
            phy_write(
                regs,
                MII_BMCR,
                bmcr | BMCR_ANENABLE | BMCR_ANRESTART | BMCR_SPEED1000 | BMCR_FULL_DUPLEX,
            )
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::{FakeRegs, BMSR_ANEG_COMPLETE, BMSR_LINK, MII_BMSR};

    /// Link and completed autoneg as the PHY reports them (`BMSR` link status
    /// is latched-low, so the second read is the current state).
    fn phy_link_up(regs: &FakeRegs) -> ViResult<bool> {
        let _ = phy_read(regs, MII_BMSR)?;
        let bmsr = phy_read(regs, MII_BMSR)?;
        Ok(bmsr & (BMSR_LINK | BMSR_ANEG_COMPLETE) == (BMSR_LINK | BMSR_ANEG_COMPLETE))
    }

    #[test]
    fn mdic_command_encodes_the_82574_fields() {
        let write = mdic_command(true, MII_BMCR, 0x1340);
        assert_eq!(write & MDIC_DATA_MASK, 0x1340);
        assert_eq!((write >> MDIC_REG_SHIFT) & 0x1F, 0);
        assert_eq!((write >> MDIC_PHY_SHIFT) & 0x1F, 1);
        assert_eq!((write >> 26) & 0b11, 0b01);
        let read = mdic_command(false, MII_BMSR, 0xFFFF);
        assert_eq!(read & MDIC_DATA_MASK, 0xFFFF);
        assert_eq!((read >> MDIC_REG_SHIFT) & 0x1F, 1);
        assert_eq!((read >> 26) & 0b11, 0b10);
        assert_eq!(read & (MDIC_READY | MDIC_ERROR), 0);
    }

    #[test]
    fn autoneg_restart_sets_enable_and_restart_and_link_follows() {
        let regs = FakeRegs::new();
        regs.phy.borrow_mut()[MII_BMCR as usize] = BMCR_FULL_DUPLEX;
        assert_eq!(phy_link_up(&regs), Ok(false));
        restart_autoneg(&regs, MacKind::I82574).expect("MDIC write completes");
        // The written BMCR kept the old bits and asked for autoneg.
        let written = regs
            .writes
            .borrow()
            .iter()
            .rev()
            .find(|(offset, value)| *offset == MDIC && value & MDIC_OP_WRITE != 0)
            .map(|&(_, value)| (value & MDIC_DATA_MASK) as u16)
            .expect("a BMCR write");
        assert_eq!(written & BMCR_ANENABLE, BMCR_ANENABLE);
        assert_eq!(written & BMCR_ANRESTART, BMCR_ANRESTART);
        assert_eq!(written & BMCR_FULL_DUPLEX, BMCR_FULL_DUPLEX);
        // The fake PHY completes autoneg on restart, as a real link partner would.
        assert_eq!(phy_link_up(&regs), Ok(true));
    }

    #[test]
    fn mdic_error_and_wedged_phy_fail_closed() {
        let mut regs = FakeRegs::new();
        regs.phy_address = 2;
        assert_eq!(phy_read(&regs, MII_BMSR), Err(ViError::IO));
        regs.phy_address = PHY_ADDR;
        regs.responsive = false;
        assert_eq!(phy_write(&regs, MII_BMCR, 0), Err(ViError::IO));
    }

    #[test]
    fn pch_lan_autoneg_restart_acquires_and_releases_semaphore() {
        let regs = FakeRegs::new();
        regs.phy.borrow_mut()[MII_BMCR as usize] = BMCR_FULL_DUPLEX;
        restart_autoneg(
            &regs,
            MacKind::PchLan(crate::identity::PchGen::SunrisePoint),
        )
        .expect("PCH LAN autoneg succeeds");
        let writes = regs.writes.borrow();
        assert!(
            writes
                .iter()
                .any(|&(o, v)| o == crate::pch::EXTCNF_CTRL
                    && v & crate::pch::EXTCNF_CTRL_SWFLAG != 0)
        );
        assert_eq!(
            regs.get(crate::pch::EXTCNF_CTRL) & crate::pch::EXTCNF_CTRL_SWFLAG,
            0
        );
    }
}
