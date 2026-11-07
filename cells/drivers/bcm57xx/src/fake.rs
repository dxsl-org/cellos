//! Host tests only: fake Broadcom register file with APE and MI_COMM emulation.

use crate::ape::{APE_PER_LOCK_GRANT, APE_PER_LOCK_REQ};
use crate::regs::{RegisterFile, MICOMM_BUSY, MISCCFG_RESET_CORE_CLOCKS, MISC_CFG, MI_COMM};
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
            o if o >= APE_PER_LOCK_REQ && o < APE_PER_LOCK_REQ + 32 && self.responsive => {
                let grant_off = o - APE_PER_LOCK_REQ + APE_PER_LOCK_GRANT;
                self.regs.borrow_mut().insert(grant_off, value);
                value
            }
            o if o >= APE_PER_LOCK_GRANT && o < APE_PER_LOCK_GRANT + 32 && self.responsive => {
                // Unlock clears grant
                0
            }
            MISC_CFG if self.responsive => value & !MISCCFG_RESET_CORE_CLOCKS,
            MI_COMM if self.responsive => value & !MICOMM_BUSY,
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
    fn fake_regs_ape_lock() {
        let fake = FakeRegs::new();
        fake.wr32(APE_PER_LOCK_REQ, 0x1000).unwrap();
        assert_eq!(fake.rd32(APE_PER_LOCK_GRANT).unwrap(), 0x1000);
        fake.wr32(APE_PER_LOCK_GRANT, 0x1000).unwrap();
        assert_eq!(fake.rd32(APE_PER_LOCK_GRANT).unwrap(), 0);
    }
}
