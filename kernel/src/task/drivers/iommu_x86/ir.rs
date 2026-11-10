//! VT-d Interrupt Remapping formats (Intel VT-d spec §§5.1, 9.10).
//!
//! Tables run in xAPIC format (`IRTA.EIME` = 0): destinations are 8-bit APIC
//! IDs, matching the kernel's I/O APIC and MSI targets. `GCMD.CFI` stays
//! clear, so a remapping unit blocks compatibility-format interrupts; every
//! I/O APIC entry and MSI behind it must use the remappable formats below.

pub mod table;

use core::sync::atomic::{AtomicBool, Ordering};

pub const DMAR_IRTA_REG: usize = 0xB8;

pub const VTD_GCMD_IRE: u32 = 1 << 25;
pub const VTD_GCMD_SIRTP: u32 = 1 << 24;

pub const VTD_GSTS_IRES: u32 = 1 << 25;
pub const VTD_GSTS_IRTPS: u32 = 1 << 24;

/// `ECAP.IR`: the unit supports interrupt remapping.
pub const ECAP_IR: u64 = 1 << 3;

static IR_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Whether at least one unit has interrupt remapping enabled.
pub fn is_ir_active() -> bool {
    IR_ACTIVE.load(Ordering::Acquire)
}

pub fn set_ir_active(active: bool) {
    IR_ACTIVE.store(active, Ordering::Release);
}

/// 128-bit Interrupt Remapping Table Entry.
#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Irte {
    pub lo: u64,
    pub hi: u64,
}

impl Irte {
    /// Present, remapped-format entry: fixed delivery, physical destination
    /// `dest_apic_id` in the xAPIC field (bits 47:40), requester `sid`
    /// verified exactly (SVT=01, SQ=00).
    pub fn encode(vector: u8, dest_apic_id: u8, sid: u16, level: bool) -> Self {
        let trigger = if level { 1u64 << 4 } else { 0 };
        let lo = 1u64 | trigger | ((vector as u64) << 16) | ((dest_apic_id as u64) << 40);
        let hi = (sid as u64) | (1u64 << 18);
        Self { lo, hi }
    }
}

/// Remappable-format MSI address for `handle`. With `subhandle` set the
/// hardware adds the message data (the device's vector offset within a
/// multi-message MSI block) to the handle.
pub fn remappable_msi_address(handle: u16, subhandle: bool) -> u64 {
    let index_l = (handle & 0x7FFF) as u64;
    let index_h = ((handle >> 15) & 1) as u64;
    let shv = if subhandle { 1u64 << 3 } else { 0 };
    0xFEE0_0000u64 | (1 << 4) | (index_l << 5) | (index_h << 2) | shv
}

/// Message data for a remappable MSI: subhandle base 0.
pub fn remappable_msi_data() -> u32 {
    0
}

/// Remappable-format I/O APIC redirection entry (§5.1.5.1) naming IRTE
/// `index`, unmasked. `vector` repeats the IRTE's vector: the I/O APIC
/// matches EOI broadcasts for level-triggered pins against this field.
pub fn remappable_ioapic_entry(index: u16, vector: u8, level: bool, active_low: bool) -> u64 {
    let index_l = (index & 0x7FFF) as u64;
    let index_h = ((index >> 15) & 1) as u64;
    let trigger = (level as u64) << 15;
    let polarity = (active_low as u64) << 13;
    (index_l << 49) | (1u64 << 48) | (index_h << 11) | trigger | polarity | vector as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn irte_encoding_and_fields() {
        let entry = Irte::encode(0x42, 0x05, 0x0100, false);
        assert_eq!(entry.lo & 1, 1); // present
        assert_eq!((entry.lo >> 4) & 1, 0); // edge
        assert_eq!((entry.lo >> 16) & 0xFF, 0x42); // vector
        assert_eq!(entry.lo >> 32, 0x0500); // xAPIC dest_id in bits 47:40
        assert_eq!(entry.hi & 0xFFFF, 0x0100); // sid
        assert_eq!((entry.hi >> 16) & 0x3, 0); // sq: exact
        assert_eq!((entry.hi >> 18) & 0x3, 1); // sid_vtype
        let level = Irte::encode(0x2E, 0, 0xF0F8, true);
        assert_eq!(level.lo, 0x002E_0011);
        assert_eq!(level.hi, 0x4_F0F8);
    }

    #[test]
    fn remappable_msi_address_encoding() {
        let addr = remappable_msi_address(0x123, false);
        assert_eq!(addr & 0xFFF0_0000, 0xFEE0_0000);
        assert_eq!((addr >> 4) & 1, 1); // int_mode
        assert_eq!((addr >> 3) & 1, 0); // no subhandle
        assert_eq!((addr >> 5) & 0x7FFF, 0x123);
        assert_eq!((addr >> 2) & 1, 0);

        let addr_hi = remappable_msi_address(0x8005, true);
        assert_eq!((addr_hi >> 5) & 0x7FFF, 5);
        assert_eq!((addr_hi >> 2) & 1, 1);
        assert_eq!((addr_hi >> 3) & 1, 1);
    }

    #[test]
    fn remappable_ioapic_entry_encoding() {
        // Edge, active-high: format bit 48, index in 63:49, vector in 7:0.
        assert_eq!(
            remappable_ioapic_entry(3, 0x24, false, false),
            (3u64 << 49) | (1 << 48) | 0x24
        );
        let entry = remappable_ioapic_entry(0x8001, 0x2E, true, true);
        assert_eq!(entry >> 49, 1); // index[14:0]
        assert_eq!((entry >> 48) & 1, 1); // remappable format
        assert_eq!((entry >> 16) & 1, 0); // unmasked
        assert_eq!((entry >> 15) & 1, 1); // level
        assert_eq!((entry >> 13) & 1, 1); // active low
        assert_eq!((entry >> 11) & 1, 1); // index[15]
        assert_eq!((entry >> 8) & 0x7, 0); // delivery field must be 000
        assert_eq!(entry & 0xFF, 0x2E);
    }
}
