#![allow(dead_code)]
//! USB 2.0 Hub Class support (USB 2.0 spec chapter 11).

pub const USB_CLASS_HUB: u8 = 0x09;

// Hub Class Requests
pub const USB_REQ_GET_STATUS: u8 = 0x00;
pub const USB_REQ_CLEAR_FEATURE: u8 = 0x01;
pub const USB_REQ_SET_FEATURE: u8 = 0x03;
pub const USB_REQ_GET_DESCRIPTOR: u8 = 0x06;

pub const BM_REQ_HUB_GET_DESC: u8 = 0xA0; // Class | Device | In
pub const BM_REQ_HUB_CLASS_OTHER: u8 = 0x23; // Class | Other | Out
pub const BM_REQ_HUB_PORT_STATUS: u8 = 0xA3; // Class | Other | In

pub const USB_DT_HUB: u16 = 0x2900;

// Port Features (USB 2.0 Table 11-17)
pub const PORT_CONNECTION: u16 = 0;
pub const PORT_ENABLE: u16 = 1;
pub const PORT_SUSPEND: u16 = 2;
pub const PORT_OVER_CURRENT: u16 = 3;
pub const PORT_RESET: u16 = 4;
pub const PORT_POWER: u16 = 8;
pub const PORT_LOW_SPEED: u16 = 9;
pub const C_PORT_CONNECTION: u16 = 16;
pub const C_PORT_RESET: u16 = 20;

// Port Status Bits (USB 2.0 Table 11-21)
pub const PORT_STAT_CONNECTION: u16 = 1 << 0;
pub const PORT_STAT_ENABLE: u16 = 1 << 1;
pub const PORT_STAT_RESET: u16 = 1 << 4;
pub const PORT_STAT_POWER: u16 = 1 << 8;
pub const PORT_STAT_LOW_SPEED: u16 = 1 << 9;
pub const PORT_STAT_HIGH_SPEED: u16 = 1 << 10;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HubDescriptor {
    pub num_ports: u8,
    pub characteristics: u16,
    pub power_on_to_power_good_2ms: u8,
}

impl HubDescriptor {
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 7 || bytes[1] != 0x29 {
            return None;
        }
        Some(Self {
            num_ports: bytes[2],
            characteristics: u16::from_le_bytes([bytes[3], bytes[4]]),
            power_on_to_power_good_2ms: bytes[5],
        })
    }
}

/// Decode device speed from hub wPortStatus word.
pub fn port_speed_from_status(w_status: u16) -> u32 {
    if w_status & PORT_STAT_LOW_SPEED != 0 {
        2 // SPEED_LOW
    } else if w_status & PORT_STAT_HIGH_SPEED != 0 {
        3 // SPEED_HIGH
    } else {
        1 // SPEED_FULL
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hub_descriptor() {
        // Standard 4-port USB 2.0 hub descriptor
        let desc = [0x09, 0x29, 0x04, 0x09, 0x00, 0x32, 0x00, 0xff];
        let parsed = HubDescriptor::parse(&desc).expect("parse hub");
        assert_eq!(parsed.num_ports, 4);
        assert_eq!(parsed.characteristics, 0x0009);
        assert_eq!(parsed.power_on_to_power_good_2ms, 0x32);
    }

    #[test]
    fn decode_speed_from_status() {
        // Full speed
        assert_eq!(
            port_speed_from_status(PORT_STAT_CONNECTION | PORT_STAT_ENABLE),
            1
        );
        // Low speed
        assert_eq!(
            port_speed_from_status(PORT_STAT_CONNECTION | PORT_STAT_ENABLE | PORT_STAT_LOW_SPEED),
            2
        );
        // High speed
        assert_eq!(
            port_speed_from_status(PORT_STAT_CONNECTION | PORT_STAT_ENABLE | PORT_STAT_HIGH_SPEED),
            3
        );
    }
}
