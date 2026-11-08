//! Host test fixture simulating Intel iGPU MMIO registers and forcewake handshakes.

use crate::regs::{
    RegisterFile, FORCEWAKE_ACK_BIT, FORCEWAKE_ACK_GT, FORCEWAKE_GT, GMD_ID, RPM_CONFIG0,
    XE2_GT_GEOMETRY_DSS_1, XE2_GT_GEOMETRY_DSS_2, XELP_EU_ENABLE,
};
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::cell::RefCell;
use types::ViResult;

pub struct FakeRegs {
    pub storage: RefCell<BTreeMap<usize, u32>>,
    pub writes: RefCell<Vec<(usize, u32)>>,
    pub auto_ack_forcewake: bool,
}

impl FakeRegs {
    pub fn new() -> Self {
        Self {
            storage: RefCell::new(BTreeMap::new()),
            writes: RefCell::new(Vec::new()),
            auto_ack_forcewake: true,
        }
    }

    /// Preconfigure registers for an Intel Panther Lake iGPU (Xe3).
    pub fn for_panther_lake() -> Self {
        let fake = Self::new();
        // GMD_ID: major 30, minor 0, rev 0
        fake.set(GMD_ID, (30 << 22) | (0 << 14) | (0 << 6));
        // RPM_CONFIG0: crystal 24 MHz (code 0)
        fake.set(RPM_CONFIG0, 0);
        // DSS mask: 12 DSS enabled (0xFFF)
        fake.set(XE2_GT_GEOMETRY_DSS_1, 0x0FFF);
        fake.set(XE2_GT_GEOMETRY_DSS_2, 0);
        // EU enable: 0xFF
        fake.set(XELP_EU_ENABLE, 0xFF);
        fake.set(
            crate::guc::GUC_STATUS,
            crate::guc::GUC_STATUS_BOOTLOADER_READY,
        );
        fake
    }

    /// Preconfigure registers for an Intel Lunar Lake iGPU (Xe2).
    pub fn for_lunar_lake() -> Self {
        let fake = Self::new();
        // GMD_ID: major 20, minor 4, rev 0
        fake.set(GMD_ID, (20 << 22) | (4 << 14) | (0 << 6));
        // RPM_CONFIG0: crystal 38.4 MHz (code 2) -> (2 << 3)
        fake.set(RPM_CONFIG0, 2 << 3);
        fake.set(XE2_GT_GEOMETRY_DSS_1, 0x00FF);
        fake.set(XE2_GT_GEOMETRY_DSS_2, 0);
        fake.set(XELP_EU_ENABLE, 0xFF);
        fake.set(
            crate::guc::GUC_STATUS,
            crate::guc::GUC_STATUS_BOOTLOADER_READY,
        );
        fake
    }

    pub fn set(&self, offset: usize, value: u32) {
        self.storage.borrow_mut().insert(offset, value);
    }

    pub fn get(&self, offset: usize) -> u32 {
        *self.storage.borrow().get(&offset).unwrap_or(&0)
    }
}

impl RegisterFile for FakeRegs {
    fn read32(&self, offset: usize) -> ViResult<u32> {
        Ok(self.get(offset))
    }

    fn write32(&mut self, offset: usize, value: u32) -> ViResult<()> {
        self.writes.borrow_mut().push((offset, value));
        self.set(offset, value);

        // Simulate Forcewake ACK
        if offset == FORCEWAKE_GT && self.auto_ack_forcewake {
            let mask = (value >> 16) & 0xFFFF;
            let val = value & 0xFFFF;
            if mask & 1 != 0 {
                let current_ack = self.get(FORCEWAKE_ACK_GT);
                if val & 1 != 0 {
                    self.set(FORCEWAKE_ACK_GT, current_ack | FORCEWAKE_ACK_BIT);
                } else {
                    self.set(FORCEWAKE_ACK_GT, current_ack & !FORCEWAKE_ACK_BIT);
                }
            }
        }
        // Simulate GuC DMA transfer completion
        if offset == crate::guc::GUC_DMA_CTRL && (value & crate::guc::GUC_DMA_CTRL_START) != 0 {
            // DMA finishes immediately in simulation
            self.set(crate::guc::GUC_DMA_CTRL, 0);
            self.set(crate::guc::GUC_STATUS, crate::guc::GUC_STATUS_UKERNEL_READY);
        }

        Ok(())
    }
}
