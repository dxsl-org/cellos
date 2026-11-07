//! Broadcom NetXtreme BCM5719/BCM5720 (`tg3`/`bge`) PCI identity table.
//!
//! Broadcom NetXtreme BCM5719/5720 Programmer's Reference Guide; FreeBSD `if_bgereg.h`.

pub const VENDOR_BROADCOM: u16 = 0x14E4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BcmModel {
    Bcm5720,
    Bcm5719,
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
    pub model: BcmModel,
    pub admission: Admission,
    pub name: &'static str,
}

const fn id(device_id: u16, model: BcmModel, admission: Admission, name: &'static str) -> Identity {
    Identity {
        device_id,
        model,
        admission,
        name,
    }
}

use Admission::{Driveable, Target};
use BcmModel::{Bcm5719, Bcm5720};

pub const IDENTITIES: [Identity; 2] = [
    id(0x165F, Bcm5720, Target, "BCM5720 2-port LOM"),
    id(0x1657, Bcm5719, Target, "BCM5719 4-port LOM"),
];

/// Initial Driveable set is empty until physical capture per contract.
pub const QUERY_DEVICE_IDS: [u16; 0] = [];

pub const fn lookup(vendor_id: u16, device_id: u16) -> Option<Identity> {
    if vendor_id != VENDOR_BROADCOM {
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
    fn known_bcm_skus_are_recognized() {
        let bcm5720 = lookup(VENDOR_BROADCOM, 0x165F).expect("BCM5720 is listed");
        assert_eq!(bcm5720.model, BcmModel::Bcm5720);
        assert_eq!(bcm5720.admission, Admission::Target);

        let bcm5719 = lookup(VENDOR_BROADCOM, 0x1657).expect("BCM5719 is listed");
        assert_eq!(bcm5719.model, BcmModel::Bcm5719);
    }

    #[test]
    fn unadmitted_skus_fail_closed() {
        assert_eq!(classify(VENDOR_BROADCOM, 0x165F), None);
        assert_eq!(classify(0x8086, 0x165F), None);
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
