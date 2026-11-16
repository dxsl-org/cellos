//! Host tests only: fake Intel X710 register file with Admin Queue responder.

use crate::regs::{RegisterFile, GLGEN_RST, GLGEN_RST_DEV_ARM, PF_ATQH, PF_ATQT};
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

impl RegisterFile for FakeRegs {
    fn rd32(&self, offset: usize) -> ViResult<u32> {
        Ok(self.get(offset))
    }

    fn wr32(&self, offset: usize, value: u32) -> ViResult<()> {
        self.writes.borrow_mut().push((offset, value));
        let stored = match offset {
            GLGEN_RST if self.responsive => value & !GLGEN_RST_DEV_ARM,
            PF_ATQT if self.responsive => {
                // Advance head to simulate firmware completion
                self.regs.borrow_mut().insert(PF_ATQH, value);
                value
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

    #[test]
    fn fake_regs_reset_clears_arm() {
        let fake = FakeRegs::new();
        fake.wr32(GLGEN_RST, GLGEN_RST_DEV_ARM).unwrap();
        assert_eq!(fake.rd32(GLGEN_RST).unwrap() & GLGEN_RST_DEV_ARM, 0);
    }
}
