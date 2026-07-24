//! Emulated MC146818 CMOS/RTC (index port 0x70, data port 0x71).
//!
//! Linux polls the RTC early (status register A's UIP bit, then the time
//! registers). Without a model the guest spins forever on port 0x70/0x71, so
//! this returns a stable, "update-not-in-progress", valid-battery RTC with a
//! fixed epoch — good enough to let boot proceed (the wall clock is corrected
//! later by NTP in a real deployment).

/// CMOS register indices.
const REG_STATUS_A: u8 = 0x0A;
const REG_STATUS_B: u8 = 0x0B;
const REG_STATUS_C: u8 = 0x0C;
const REG_STATUS_D: u8 = 0x0D;

/// Minimal RTC: just the selected index (NMI-disable bit masked off).
pub struct CmosRtc {
    index: u8,
}

impl CmosRtc {
    pub const fn new() -> Self {
        Self { index: 0 }
    }

    pub fn owns(port: u16) -> bool {
        matches!(port, 0x70 | 0x71)
    }

    pub fn write(&mut self, port: u16, val: u32) {
        if port == 0x70 {
            self.index = (val & 0x7F) as u8; // low 7 bits = register; bit7 = NMI mask
        }
        // Writes to 0x71 (setting RTC) are ignored — the guest's clock is virtual.
    }

    pub fn read(&self, port: u16) -> u32 {
        if port != 0x71 {
            return 0;
        }
        let v: u8 = match self.index {
            REG_STATUS_A => 0x26, // UIP=0 (not updating), 32 kHz base, rate 6
            REG_STATUS_B => 0x02, // 24-hour mode, BCD
            REG_STATUS_C => 0x00, // no interrupt flags pending
            REG_STATUS_D => 0x80, // battery/RAM valid
            _ => 0,               // time/date registers → fixed zero epoch
        };
        v as u32
    }
}
