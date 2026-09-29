//! Software GICv2 CPU interface for boards without GICH/GICV.

use crate::gicd::Gicd;

pub struct Gicc {
    ctlr: u32,
    pmr: u8,
    bpr: u8,
    active: [u32; 8],
}

impl Gicc {
    pub const fn new() -> Self {
        Self {
            ctlr: 0,
            pmr: 0,
            bpr: 0,
            active: [0; 8],
        }
    }

    fn running_priority(&self, distributor: &Gicd) -> u8 {
        let mut best = 0xff;
        for (word, &mask) in self.active.iter().enumerate() {
            let mut mask = mask;
            while mask != 0 {
                let bit = mask.trailing_zeros();
                best = best.min(distributor.priority((word * 32) as u32 + bit));
                mask &= mask - 1;
            }
        }
        best
    }

    fn next(&self, distributor: &Gicd) -> Option<u32> {
        if self.ctlr & 1 == 0 {
            return None;
        }
        distributor.highest_pending(self.pmr, self.running_priority(distributor))
    }

    pub fn pending_irq(&self, distributor: &Gicd) -> Option<u32> {
        self.next(distributor)
    }

    pub fn read(&mut self, distributor: &mut Gicd, offset: u64) -> u64 {
        match offset {
            0x000 => self.ctlr as u64,
            0x004 => self.pmr as u64,
            0x008 => self.bpr as u64,
            0x00c => {
                if let Some(intid) = self.next(distributor) {
                    distributor.acknowledge(intid);
                    self.active[intid as usize / 32] |= 1 << (intid % 32);
                    intid as u64
                } else {
                    1023 // GICv2 spurious interrupt ID
                }
            }
            0x014 => self.running_priority(distributor) as u64,
            0x018 => self.next(distributor).unwrap_or(1023) as u64,
            0x0fc => 0x0202_043b, // ARM GICv2 CPU interface
            _ => 0,
        }
    }

    pub fn write(&mut self, distributor: &mut Gicd, offset: u64, val: u64) {
        match offset {
            0x000 => self.ctlr = val as u32 & 1,
            0x004 => self.pmr = val as u8,
            0x008 => self.bpr = val as u8 & 7,
            0x010 => {
                let intid = val as u32 & 0x3ff;
                let word = intid as usize / 32;
                if word < self.active.len() && self.active[word] & (1 << (intid % 32)) != 0 {
                    self.active[word] &= !(1 << (intid % 32));
                    distributor.eoi(intid);
                }
            }
            _ => {}
        }
    }
}
