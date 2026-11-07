//! Intel X710/XL710 (`i40e`) PCI identity table.
//!
//! Intel Ethernet Controller X710/XXV710/XL710 Datasheet §6.1; FreeBSD `i40e_devids.h`.

pub const VENDOR_INTEL: u16 = 0x8086;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum I40eModel {
    X710,
    XL710,
    XXV710,
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
    pub model: I40eModel,
    pub admission: Admission,
    pub name: &'static str,
}

const fn id(
    device_id: u16,
    model: I40eModel,
    admission: Admission,
    name: &'static str,
) -> Identity {
    Identity {
        device_id,
        model,
        admission,
        name,
    }
}

use Admission::{Driveable, Target};
use I40eModel::{X710, XL710, XXV710};

pub const IDENTITIES: [Identity; 5] = [
    id(0x1572, X710, Target, "X710 10G SFP+"),
    id(0x1580, XL710, Target, "XL710 40G QSFP+"),
    id(0x1581, XL710, Target, "XL710 40G Backplane"),
    id(0x1586, X710, Target, "X710-T 10GBASE-T"),
    id(0x1589, XXV710, Target, "XXV710 25G SFP28"),
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
    fn known_i40e_skus_are_recognized() {
        let x710 = lookup(VENDOR_INTEL, 0x1572).expect("X710 is listed");
        assert_eq!(x710.model, I40eModel::X710);
        assert_eq!(x710.admission, Admission::Target);

        let xl710 = lookup(VENDOR_INTEL, 0x1580).expect("XL710 is listed");
        assert_eq!(xl710.model, I40eModel::XL710);

        let xxv = lookup(VENDOR_INTEL, 0x1589).expect("XXV710 is listed");
        assert_eq!(xxv.model, I40eModel::XXV710);
    }

    #[test]
    fn unadmitted_skus_fail_closed() {
        assert_eq!(classify(VENDOR_INTEL, 0x1572), None);
        assert_eq!(classify(VENDOR_INTEL, 0x1580), None);
        assert_eq!(classify(0x10EC, 0x1572), None);
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
