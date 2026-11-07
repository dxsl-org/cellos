//! Host tests only: fake ixgbe register file with EERD and SFP I2C emulation.

use crate::regs::{
    RegisterFile, CTRL, CTRL_RST, EERD, EERD_ADDR_SHIFT, EERD_DATA_SHIFT, EERD_DONE, EERD_START,
    I2CCTL_82599, I2C_CLK_IN, I2C_CLK_OUT, I2C_DATA_IN, I2C_DATA_OUT,
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
            CTRL if self.responsive => value & !CTRL_RST,
            I2CCTL_82599 if self.responsive => {
                // Echo outputs back to inputs for bit-banging test
                let mut res = value;
                if value & I2C_CLK_OUT != 0 {
                    res |= I2C_CLK_IN;
                } else {
                    res &= !I2C_CLK_IN;
                }
                if value & I2C_DATA_OUT != 0 {
                    res |= I2C_DATA_IN;
                } else {
                    res &= !I2C_DATA_IN;
                }
                res
            }
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
        fake.nvm[3] = 0x5678;
        assert_eq!(nvm_read(&fake, 3).unwrap(), 0x5678);
    }
}
