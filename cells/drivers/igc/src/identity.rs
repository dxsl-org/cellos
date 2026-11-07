//! Intel I225/I226 (`igc`) PCI identity table.
//!
//! Intel Ethernet Controller I225/I226 Datasheets §6.2.5.
//! Every SKU is `Target` until physical capture on that specific device ID.

pub const VENDOR_INTEL: u16 = 0x8086;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IgcModel {
    I225,
    I226,
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
    pub model: IgcModel,
    pub admission: Admission,
    pub name: &'static str,
}

const fn id(device_id: u16, model: IgcModel, admission: Admission, name: &'static str) -> Identity {
    Identity {
        device_id,
        model,
        admission,
        name,
    }
}

use Admission::{Driveable, Target};
use IgcModel::{I225, I226};

pub const IDENTITIES: [Identity; 8] = [
    id(0x15F2, I225, Target, "I225-LM"),
    id(0x15F3, I225, Target, "I225-V"),
    id(0x15F8, I225, Target, "I225-I"),
    id(0x3100, I225, Target, "I225-K"),
    id(0x125B, I226, Target, "I226-LM"),
    id(0x125C, I226, Target, "I226-V"),
    id(0x125D, I226, Target, "I226-IT"),
    id(0x3102, I226, Target, "I226-K"),
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
    fn known_i225_and_i226_skus_are_recognized() {
        let i225_v = lookup(VENDOR_INTEL, 0x15F3).expect("I225-V is listed");
        assert_eq!(i225_v.model, IgcModel::I225);
        assert_eq!(i225_v.admission, Admission::Target);
        assert_eq!(i225_v.name, "I225-V");

        let i226_v = lookup(VENDOR_INTEL, 0x125C).expect("I226-V is listed");
        assert_eq!(i226_v.model, IgcModel::I226);
        assert_eq!(i226_v.admission, Admission::Target);
        assert_eq!(i226_v.name, "I226-V");

        let i226_lm = lookup(VENDOR_INTEL, 0x125B).expect("I226-LM is listed");
        assert_eq!(i226_lm.model, IgcModel::I226);
    }

    #[test]
    fn unadmitted_skus_fail_closed() {
        assert_eq!(classify(VENDOR_INTEL, 0x15F3), None);
        assert_eq!(classify(VENDOR_INTEL, 0x125C), None);
        assert_eq!(classify(VENDOR_INTEL, 0x10D3), None);
        assert_eq!(classify(0x10EC, 0x15F3), None);
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
