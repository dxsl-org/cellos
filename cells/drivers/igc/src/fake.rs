//! Host tests only: fake I225/I226 register file with EERD and MDIC emulation.

use crate::phy::{MDIC_DATA_MASK, MDIC_OP_WRITE, MDIC_READY};
use crate::regs::{
    RegisterFile, CTRL, CTRL_RST, EERD, EERD_ADDR_SHIFT, EERD_DATA_SHIFT, EERD_DONE, EERD_START,
    MDIC, SWSM, SWSM_SMBI, SW_FW_SYNC,
};
use std::cell::RefCell;
use std::collections::BTreeMap;
use types::ViResult;

pub(crate) struct FakeRegs {
    pub regs: RefCell<BTreeMap<usize, u32>>,
    pub nvm: [u16; 64],
    pub responsive: bool,
    pub writes: RefCell<Vec<(usize, u32)>>,
}

impl FakeRegs {
    pub(crate) fn new() -> Self {
        Self {
            regs: RefCell::new(BTreeMap::new()),
            nvm: [0; 64],
            responsive: true,
            writes: RefCell::new(Vec::new()),
        }
    }

    pub(crate) fn get(&self, offset: usize) -> u32 {
        *self.regs.borrow().get(&offset).unwrap_or(&0)
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
            MDIC if self.responsive => value | MDIC_READY,
            CTRL if self.responsive => value & !CTRL_RST,
            SWSM if self.responsive => {
                let cur = self.get(SWSM);
                if value & SWSM_SMBI != 0 && cur & SWSM_SMBI == 0 {
                    value | SWSM_SMBI
                } else if value & SWSM_SMBI == 0 {
                    cur & !SWSM_SMBI
                } else {
                    cur
                }
            }
            SW_FW_SYNC if self.responsive => value,
            _ => value,
        };
        self.regs.borrow_mut().insert(offset, stored);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::regs::nvm_read;

    #[test]
    fn fake_regs_nvm_read() {
        let mut fake = FakeRegs::new();
        fake.nvm[2] = 0xABCD;
        let val = nvm_read(&fake, 2).expect("nvm_read succeeds");
        assert_eq!(val, 0xABCD);
    }
}
