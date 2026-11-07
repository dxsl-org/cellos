//! Realtek RTL8111/RTL8168 and RTL8125 PCI identity and XID decoding table.
//!
//! Realtek RTL8111/8168 and RTL8125 datasheets; FreeBSD `if_rlreg.h`.

pub const VENDOR_REALTEK: u16 = 0x10EC;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RtlModel {
    Rtl8168,
    Rtl8125,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RtlXid {
    Rtl8168B,
    Rtl8168C,
    Rtl8168D,
    Rtl8168E,
    Rtl8168F,
    Rtl8168G,
    Rtl8168H,
    Rtl8125A,
    Rtl8125B,
    Unknown(u32),
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
    pub model: RtlModel,
    pub admission: Admission,
    pub name: &'static str,
}

const fn id(device_id: u16, model: RtlModel, admission: Admission, name: &'static str) -> Identity {
    Identity {
        device_id,
        model,
        admission,
        name,
    }
}

use Admission::{Driveable, Target};
use RtlModel::{Rtl8125, Rtl8168};

pub const IDENTITIES: [Identity; 3] = [
    id(0x8168, Rtl8168, Target, "RTL8168/8111"),
    id(0x8161, Rtl8168, Target, "RTL8111G/EP"),
    id(0x8125, Rtl8125, Target, "RTL8125 2.5G"),
];

/// Initial Driveable set is empty until physical capture per contract.
pub const QUERY_DEVICE_IDS: [u16; 0] = [];

pub const fn lookup(vendor_id: u16, device_id: u16) -> Option<Identity> {
    if vendor_id != VENDOR_REALTEK {
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

/// Decode XID from the hardware `TxConfig` register (bits 30..22).
pub fn decode_xid(txcfg: u32) -> RtlXid {
    let xid = txcfg & 0x7CC0_0000;
    match xid {
        0x3000_0000 => RtlXid::Rtl8168B,
        0x3C00_0000 | 0x3C80_0000 => RtlXid::Rtl8168C,
        0x2800_0000 | 0x2880_0000 => RtlXid::Rtl8168D,
        0x2C00_0000 | 0x2C80_0000 => RtlXid::Rtl8168E,
        0x4800_0000 => RtlXid::Rtl8168F,
        0x4C00_0000 => RtlXid::Rtl8168G,
        0x5400_0000 | 0x5480_0000 => RtlXid::Rtl8168H,
        0x6080_0000 => RtlXid::Rtl8125A,
        0x6400_0000 => RtlXid::Rtl8125B,
        _ => RtlXid::Unknown(xid),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_realtek_skus_are_recognized() {
        let r8168 = lookup(VENDOR_REALTEK, 0x8168).expect("RTL8168 is listed");
        assert_eq!(r8168.model, RtlModel::Rtl8168);
        assert_eq!(r8168.admission, Admission::Target);

        let r8125 = lookup(VENDOR_REALTEK, 0x8125).expect("RTL8125 is listed");
        assert_eq!(r8125.model, RtlModel::Rtl8125);
    }

    #[test]
    fn xid_decoding_classifies_major_steppings() {
        assert_eq!(decode_xid(0x5400_0000), RtlXid::Rtl8168H);
        assert_eq!(decode_xid(0x4C00_0000), RtlXid::Rtl8168G);
        assert_eq!(decode_xid(0x2800_0000), RtlXid::Rtl8168D);
        assert_eq!(decode_xid(0x6400_0000), RtlXid::Rtl8125B);
        assert_eq!(decode_xid(0x1234_0000), RtlXid::Unknown(0x1000_0000));
    }

    #[test]
    fn unadmitted_skus_fail_closed() {
        assert_eq!(classify(VENDOR_REALTEK, 0x8168), None);
        assert_eq!(classify(0x8086, 0x8168), None);
    }
}
