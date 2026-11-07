//! Host tests only: a fake 82574 register file — plain storage plus the `EERD`
//! and `MDIC` handshakes, so NVM and PHY code run against device-shaped
//! behaviour instead of a recording of the driver's own writes.

use crate::pch;
use crate::phy::{
    BMCR_ANENABLE, BMCR_ANRESTART, MDIC_DATA_MASK, MDIC_ERROR, MDIC_OP_READ, MDIC_OP_WRITE,
    MDIC_PHY_SHIFT, MDIC_READY, MDIC_REG_SHIFT, MII_BMCR,
};
use crate::regs::{
    RegisterFile, CTRL, CTRL_GIO_MASTER_DISABLE, CTRL_RST, EERD, EERD_ADDR_SHIFT, EERD_DATA_SHIFT,
    EERD_DONE, EERD_START, MDIC, STATUS, STATUS_GIO_MASTER_ENABLE,
};
use std::cell::RefCell;
use std::collections::BTreeMap;
use types::ViResult;

/// IEEE 802.3 clause 22: Basic Mode Status Register and its link bits.
pub(crate) const MII_BMSR: u32 = 1;
pub(crate) const BMSR_LINK: u16 = 1 << 2;
pub(crate) const BMSR_ANEG_COMPLETE: u16 = 1 << 5;

pub(crate) struct FakeRegs {
    pub regs: RefCell<BTreeMap<usize, u32>>,
    pub nvm: [u16; 64],
    /// Clause-22 registers of the PHY.
    pub phy: RefCell<[u16; 32]>,
    /// The MDIO address the PHY answers at (the 82574's is 1).
    pub phy_address: u32,
    /// When false, `EERD`/`MDIC` never complete (a wedged device).
    pub responsive: bool,
    pub writes: RefCell<Vec<(usize, u32)>>,
}

impl FakeRegs {
    pub(crate) fn new() -> Self {
        Self {
            regs: RefCell::new(BTreeMap::new()),
            nvm: [0; 64],
            phy: RefCell::new([0; 32]),
            phy_address: 1,
            responsive: true,
            writes: RefCell::new(Vec::new()),
        }
    }

    pub(crate) fn get(&self, offset: usize) -> u32 {
        *self.regs.borrow().get(&offset).unwrap_or(&0)
    }

    /// One `MDIC` transaction: the value the register reads back afterwards.
    fn mdic(&self, command: u32) -> u32 {
        let reg = ((command >> MDIC_REG_SHIFT) & 0x1F) as usize;
        let address = (command >> MDIC_PHY_SHIFT) & 0x1F;
        if address != self.phy_address {
            return command | MDIC_READY | MDIC_ERROR;
        }
        let mut phy = self.phy.borrow_mut();
        let op = command & (MDIC_OP_READ | MDIC_OP_WRITE);
        if op == MDIC_OP_WRITE {
            let data = (command & MDIC_DATA_MASK) as u16;
            phy[reg] = data & !BMCR_ANRESTART;
            // A restart with autoneg enabled completes against a link partner.
            if reg == MII_BMCR as usize && data & (BMCR_ANENABLE | BMCR_ANRESTART) != 0 {
                phy[MII_BMSR as usize] |= BMSR_LINK | BMSR_ANEG_COMPLETE;
            }
            command | MDIC_READY
        } else {
            (command & !MDIC_DATA_MASK) | u32::from(phy[reg]) | MDIC_READY
        }
    }
}

impl RegisterFile for FakeRegs {
    fn rd32(&self, offset: usize) -> ViResult<u32> {
        Ok(self.get(offset))
    }

    fn wr32(&self, offset: usize, value: u32) -> ViResult<()> {
        self.writes.borrow_mut().push((offset, value));
        let stored = match offset {
            EERD if self.responsive && value & EERD_START != 0 => {
                let word = (value >> EERD_ADDR_SHIFT) & 0x3FFF;
                let data = self.nvm.get(word as usize).copied().unwrap_or(0xFFFF);
                (u32::from(data) << EERD_DATA_SHIFT) | (value & 0xFFFC) | EERD_DONE
            }
            MDIC if self.responsive => self.mdic(value),
            // The global reset self-clears; disabling bus mastering drains
            // (at once, here) and reports it through STATUS.
            CTRL if self.responsive => {
                if value & CTRL_GIO_MASTER_DISABLE != 0 {
                    let status = self.get(STATUS) & !STATUS_GIO_MASTER_ENABLE;
                    self.regs.borrow_mut().insert(STATUS, status);
                }
                value & !CTRL_RST
            }
            pch::FLASH_HSFSTS if self.responsive && value & pch::HSFCTL_START != 0 => {
                let addr = self.get(pch::FLASH_FADDR) as usize;
                let word_idx = addr >> 1;
                let data = self.nvm.get(word_idx).copied().unwrap_or(0xFFFF);
                self.regs
                    .borrow_mut()
                    .insert(pch::FLASH_FDATA0, u32::from(data));
                pch::HSFSTS_FLCDONE
            }
            pch::H2ME if self.responsive => {
                if value & pch::H2ME_ENFORCE_SETTINGS != 0 && value & pch::H2ME_ULP == 0 {
                    let mut fwsm = self.get(pch::FWSM);
                    fwsm &= !pch::FWSM_ULP_CFG_DONE;
                    self.regs.borrow_mut().insert(pch::FWSM, fwsm);
                }
                value
            }
            _ => value,
        };
        self.regs.borrow_mut().insert(offset, stored);
        Ok(())
    }
}
