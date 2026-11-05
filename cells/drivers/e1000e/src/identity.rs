//! `/bin/e1000e` PCI identity table (`reference-nic-cell-contract.md`).
//!
//! Each entry is `Driveable` (an implemented, tested path) or `Target` (a known
//! SKU this cell's register model is meant to grow into, not admitted). Only
//! `Driveable` IDs are queried; every other ID is refused by name.
//!
//! * `8086:10D3` — 82574L (Intel 82574 GbE Controller Family Datasheet, PCI
//!   device ID table). QEMU's `e1000e` and VMware's "e1000e" emulate it, and it
//!   is q35's default NIC. The only `Driveable` entry.
//! * `8086:10F6` — 82574LA, same datasheet. `Target`: no model validates it.
//! * I219 (PCH-integrated LAN) — `Target` for phase 13: those parts need the
//!   PCH-LAN flash NVM path, ULP exit and the PHY workarounds before any of them
//!   may be admitted. IDs are the PCI ID Repository (`pci.ids`) entries for the
//!   Sunrise Point, Cannon Point, Comet Lake, Tiger Lake and Alder Lake PCHs;
//!   phase 13 re-checks them against the I219 datasheet before promotion.

/// Intel's PCI vendor ID.
pub const VENDOR_INTEL: u16 = 0x8086;

/// The MAC/PHY family a device belongs to; phase 13 adds behaviour per variant
/// without touching the 82574 paths.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MacKind {
    /// 82574L/82574LA: discrete PCIe MAC + PHY.
    I82574,
    /// I219 behind a PCH, by PCH generation.
    PchLan(PchGen),
}

/// PCH generation of an I219 connection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PchGen {
    SunrisePoint,
    CannonPoint,
    CometLake,
    TigerLake,
    AlderLake,
}

/// Whether the cell may bind an ID.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Admission {
    /// Implemented and validated end to end; queried at discovery.
    Driveable,
    /// Known SKU, not admitted; refused by name like an unknown ID.
    Target,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Identity {
    pub device_id: u16,
    pub kind: MacKind,
    pub admission: Admission,
    pub name: &'static str,
}

const fn id(device_id: u16, kind: MacKind, admission: Admission, name: &'static str) -> Identity {
    Identity {
        device_id,
        kind,
        admission,
        name,
    }
}

use Admission::{Driveable, Target};
use MacKind::{PchLan, I82574};
use PchGen::{AlderLake, CannonPoint, CometLake, SunrisePoint, TigerLake};

/// Every ID this cell knows, all under [`VENDOR_INTEL`].
pub const IDENTITIES: [Identity; 16] = [
    id(0x10D3, I82574, Driveable, "82574L"),
    id(0x10F6, I82574, Target, "82574LA"),
    id(0x156F, PchLan(SunrisePoint), Target, "I219-LM"),
    id(0x1570, PchLan(SunrisePoint), Target, "I219-V"),
    id(0x15B7, PchLan(SunrisePoint), Target, "I219-LM"),
    id(0x15B8, PchLan(SunrisePoint), Target, "I219-V"),
    id(0x15BB, PchLan(CannonPoint), Target, "I219-LM"),
    id(0x15BC, PchLan(CannonPoint), Target, "I219-V"),
    id(0x15BD, PchLan(CannonPoint), Target, "I219-LM"),
    id(0x15BE, PchLan(CannonPoint), Target, "I219-V"),
    id(0x0D4E, PchLan(CometLake), Target, "I219-LM"),
    id(0x0D4F, PchLan(CometLake), Target, "I219-V"),
    id(0x15FB, PchLan(TigerLake), Target, "I219-LM"),
    id(0x15FC, PchLan(TigerLake), Target, "I219-V"),
    id(0x1A1C, PchLan(AlderLake), Target, "I219-LM"),
    id(0x1A1D, PchLan(AlderLake), Target, "I219-V"),
];

/// The device IDs handed to `sys_find_pcie_device_by_vendor`, in order: exactly
/// the `Driveable` entries. The kernel's Ethernet gate lists the same set with
/// `/bin/e1000e` as owner.
pub const QUERY_DEVICE_IDS: [u16; 1] = [0x10D3];

const _: () = {
    // The query list is exactly the Driveable set.
    let mut driveable = 0;
    let mut i = 0;
    while i < IDENTITIES.len() {
        if matches!(IDENTITIES[i].admission, Admission::Driveable) {
            assert!(IDENTITIES[i].device_id == QUERY_DEVICE_IDS[driveable]);
            driveable += 1;
        }
        i += 1;
    }
    assert!(driveable == QUERY_DEVICE_IDS.len());
};

/// The table entry for `(vendor, device)`, whatever its admission.
pub const fn lookup(vendor_id: u16, device_id: u16) -> Option<Identity> {
    if vendor_id != VENDOR_INTEL {
        return None;
    }
    let mut i = 0;
    while i < IDENTITIES.len() {
        if IDENTITIES[i].device_id == device_id {
            return Some(IDENTITIES[i]);
        }
        i += 1;
    }
    None
}

/// The identity this cell may drive, or `None`: unknown and `Target` IDs alike
/// fail closed, and the caller names vendor:device.
pub const fn classify(vendor_id: u16, device_id: u16) -> Option<Identity> {
    match lookup(vendor_id, device_id) {
        Some(identity) if matches!(identity.admission, Admission::Driveable) => Some(identity),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_82574l_is_driveable() {
        let identity = classify(VENDOR_INTEL, 0x10D3).expect("82574L is driveable");
        assert_eq!(identity.kind, MacKind::I82574);
        assert_eq!(identity.name, "82574L");
        assert_eq!(QUERY_DEVICE_IDS, [0x10D3]);
    }

    #[test]
    fn targets_are_known_but_refused() {
        let i219 = lookup(VENDOR_INTEL, 0x15B7).expect("I219 is listed");
        assert_eq!(i219.kind, MacKind::PchLan(PchGen::SunrisePoint));
        assert_eq!(i219.admission, Admission::Target);
        assert_eq!(classify(VENDOR_INTEL, 0x15B7), None);
        assert_eq!(classify(VENDOR_INTEL, 0x10F6), None);
        for identity in IDENTITIES
            .iter()
            .filter(|i| i.admission == Admission::Target)
        {
            assert!(!QUERY_DEVICE_IDS.contains(&identity.device_id));
        }
    }

    #[test]
    fn sibling_families_and_foreign_vendors_fail_closed() {
        for device in [0x100E, 0x100F, 0x10C9, 0x1533, 0x0000, 0xFFFF] {
            assert_eq!(classify(VENDOR_INTEL, device), None, "{device:04x}");
        }
        assert_eq!(classify(0x15AD, 0x10D3), None);
        assert_eq!(classify(0x1AF4, 0x10D3), None);
    }

    #[test]
    fn table_has_no_duplicate_ids() {
        for (index, identity) in IDENTITIES.iter().enumerate() {
            assert!(IDENTITIES[index + 1..]
                .iter()
                .all(|other| other.device_id != identity.device_id));
        }
    }
}
