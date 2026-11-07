//! Intel 82599/X540/X550 (`ixgbe`) 10GbE PCI identity table.
//!
//! Intel 82599, X540, and X550 datasheets; FreeBSD `ixgbe_type.h`.

pub const VENDOR_INTEL: u16 = 0x8086;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IxgbeMac {
    Mac82599,
    MacX540,
    MacX550,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Admission {
    Driveable,
    Target,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Identity {
    pub device_id: u16,
    pub mac: IxgbeMac,
    pub admission: Admission,
    pub name: &'static str,
}

const fn id(device_id: u16, mac: IxgbeMac, admission: Admission, name: &'static str) -> Identity {
    Identity {
        device_id,
        mac,
        admission,
        name,
    }
}

use Admission::{Driveable, Target};
use IxgbeMac::{Mac82599, MacX540, MacX550};

pub const IDENTITIES: [Identity; 5] = [
    id(0x10FB, Mac82599, Target, "82599ES SFP+"),
    id(0x10F7, Mac82599, Target, "82599ES KX4"),
    id(0x1528, MacX540, Target, "X540-AT2 10GBASE-T"),
    id(0x1563, MacX550, Target, "X550-AT2 10GBASE-T"),
    id(0x15D1, MacX550, Target, "X550-T1 10GBASE-T"),
];

/// Initial Driveable set is empty until physical capture per contract.
pub const QUERY_DEVICE_IDS: [u16; 0] = [];

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

pub const fn classify(vendor_id: u16, device_id: u16) -> Option<Identity> {
    match lookup(vendor_id, device_id) {
        Some(identity) if matches!(identity.admission, Driveable) => Some(identity),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_ixgbe_skus_are_recognized() {
        let sfp = lookup(VENDOR_INTEL, 0x10FB).expect("82599ES is listed");
        assert_eq!(sfp.mac, IxgbeMac::Mac82599);
        assert_eq!(sfp.admission, Admission::Target);

        let x540 = lookup(VENDOR_INTEL, 0x1528).expect("X540 is listed");
        assert_eq!(x540.mac, IxgbeMac::MacX540);

        let x550 = lookup(VENDOR_INTEL, 0x1563).expect("X550 is listed");
        assert_eq!(x550.mac, IxgbeMac::MacX550);
    }

    #[test]
    fn unadmitted_skus_fail_closed() {
        assert_eq!(classify(VENDOR_INTEL, 0x10FB), None);
        assert_eq!(classify(VENDOR_INTEL, 0x1528), None);
        assert_eq!(classify(0x10EC, 0x10FB), None);
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
