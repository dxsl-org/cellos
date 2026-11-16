//! Host tests only: fake Realtek register file with PHYAR emulation.

use crate::regs::{RegisterFile, COMMAND, CR_RST, PHYAR, PHYAR_BUSY};
use nic_driver::regs::NarrowRegisterFile;
use std::cell::RefCell;
use std::collections::BTreeMap;
use types::ViResult;

pub(crate) struct FakeRegs {
    pub regs: RefCell<BTreeMap<usize, u32>>,
    pub responsive: bool,
    pub writes: RefCell<Vec<(usize, u32)>>,
}

impl FakeRegs {
    pub(crate) fn new() -> Self {
        Self {
            regs: RefCell::new(BTreeMap::new()),
            responsive: true,
            writes: RefCell::new(Vec::new()),
        }
    }

    pub(crate) fn get(&self, offset: usize) -> u32 {
        *self.regs.borrow().get(&offset).unwrap_or(&0)
    }
}

impl NarrowRegisterFile for FakeRegs {
    fn rd8(&self, offset: usize) -> ViResult<u8> {
        Ok((self.get(offset & !3) >> ((offset & 3) * 8)) as u8)
    }

    fn wr8(&self, offset: usize, value: u8) -> ViResult<()> {
        let base = offset & !3;
        let shift = (offset & 3) * 8;
        let mut cur = self.get(base);
        cur &= !(0xFF << shift);
        cur |= (value as u32) << shift;
        if base == (COMMAND & !3) && value & CR_RST != 0 {
            cur &= !(u32::from(CR_RST) << shift);
        }
        self.regs.borrow_mut().insert(base, cur);
        Ok(())
    }

    fn rd16(&self, offset: usize) -> ViResult<u16> {
        Ok((self.get(offset & !3) >> ((offset & 2) * 8)) as u16)
    }

    fn wr16(&self, offset: usize, value: u16) -> ViResult<()> {
        let base = offset & !3;
        let shift = (offset & 2) * 8;
        let mut cur = self.get(base);
        cur &= !(0xFFFF << shift);
        cur |= (value as u32) << shift;
        self.regs.borrow_mut().insert(base, cur);
        Ok(())
    }
}

impl RegisterFile for FakeRegs {
    fn rd32(&self, offset: usize) -> ViResult<u32> {
        Ok(self.get(offset))
    }

    fn wr32(&self, offset: usize, value: u32) -> ViResult<()> {
        self.writes.borrow_mut().push((offset, value));
        let stored = match offset {
            PHYAR if self.responsive => {
                if value & PHYAR_BUSY != 0 {
                    value & !PHYAR_BUSY
                } else {
                    value | PHYAR_BUSY
                }
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
    use crate::phy::phy_read;

    #[test]
    fn fake_regs_phy_read() {
        let fake = FakeRegs::new();
        assert!(phy_read(&fake, 0).is_ok());
    }
}
