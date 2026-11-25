//! ACPI fixed hardware named by the guest's FADT (`acpi.rs`): the PM1a event
//! and control blocks.
//!
//! No event is ever raised (there is no SCI source), so the status register
//! reads zero and the enable register only remembers what was written. The
//! control register is what the blocks exist for: with `\_S5` in the DSDT,
//! Linux powers off by setting SLP_EN in it, and that ends the VM. Without the
//! blocks `poweroff` only halted the guest ("Power off not available") and the
//! hypervisor ran on.

/// PM1a event block: status at +0, enable at +2 (16 bits each).
pub const PM1A_EVT_BLK: u16 = 0x600;
pub const PM1_EVT_LEN: u8 = 4;
/// PM1a control block (16 bits).
pub const PM1A_CNT_BLK: u16 = 0x604;
pub const PM1_CNT_LEN: u8 = 2;

const PM1_ENABLE: u16 = PM1A_EVT_BLK + 2;
/// The platform is always in ACPI mode (the FADT has no SMI command port).
const SCI_EN: u16 = 1;
/// Enter the sleep state SLP_TYP selects. The DSDT offers S5 only.
const SLP_EN: u16 = 1 << 13;

pub struct AcpiPm {
    enable: u16,
    control: u16,
}

impl AcpiPm {
    pub const fn new() -> Self {
        Self {
            enable: 0,
            control: 0,
        }
    }

    pub fn owns(port: u16) -> bool {
        (PM1A_EVT_BLK..PM1A_CNT_BLK + PM1_CNT_LEN as u16).contains(&port)
    }

    pub fn read(&self, port: u16) -> u32 {
        match port {
            PM1_ENABLE => u32::from(self.enable),
            PM1A_CNT_BLK => u32::from(self.control | SCI_EN),
            // Status: no event is ever pending.
            _ => 0,
        }
    }

    /// Returns `true` when the guest asked to power off.
    pub fn write(&mut self, port: u16, value: u32) -> bool {
        match port {
            PM1_ENABLE => self.enable = value as u16,
            PM1A_CNT_BLK => {
                self.control = value as u16 & !SLP_EN;
                return value as u16 & SLP_EN != 0;
            }
            // Status bits are write-one-to-clear and none is ever set.
            _ => {}
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_sleep_enable_powers_off() {
        let mut pm = AcpiPm::new();
        // ACPICA writes the sleep type first, then the type with SLP_EN.
        assert!(!pm.write(PM1A_CNT_BLK, 5 << 10));
        assert!(pm.write(PM1A_CNT_BLK, 5 << 10 | u32::from(SLP_EN)));
        assert!(
            !pm.write(PM1_ENABLE, 0xFFFF),
            "the enable register is not control"
        );
        assert_eq!(
            pm.read(PM1A_CNT_BLK) & u32::from(SCI_EN),
            1,
            "the guest must find ACPI mode on"
        );
    }
}
