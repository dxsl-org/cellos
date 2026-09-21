//! USB descriptor parsing (USB 2.0 §9.6).
//!
//! A configuration descriptor is a flat blob of length-prefixed sub-descriptors
//! that must be walked, not indexed: interface, endpoint, and class descriptors
//! (HID's `0x21` / `0x22`) all live inside it in device-chosen order. The walker
//! below is what makes composite devices — a wireless receiver's keyboard
//! interface plus its mouse interface — enumerable.
//!
//! Every parser validates `bLength` before advancing. A truncated or hostile
//! descriptor then ends the walk instead of driving an out-of-bounds read.

extern crate alloc;

use alloc::vec::Vec;

// ─── Descriptor types (USB 2.0 Table 9-5) ────────────────────────────────────
pub const DT_DEVICE: u8 = 0x01;
pub const DT_CONFIGURATION: u8 = 0x02;
pub const DT_STRING: u8 = 0x03;
pub const DT_INTERFACE: u8 = 0x04;
pub const DT_ENDPOINT: u8 = 0x05;
pub const DT_HID: u8 = 0x21;
pub const DT_HID_REPORT: u8 = 0x22;

// ─── Interface classes ───────────────────────────────────────────────────────
pub const CLASS_HID: u8 = 0x03;
/// Boot-interface subclass (`HID 1.11` §4.2): the device supports boot protocol.
pub const SUBCLASS_BOOT: u8 = 0x01;
pub const PROTOCOL_KEYBOARD: u8 = 0x01;
pub const PROTOCOL_MOUSE: u8 = 0x02;

// ─── Endpoint attributes ─────────────────────────────────────────────────────
pub const EP_TYPE_MASK: u8 = 0x03;
pub const EP_TYPE_CONTROL: u8 = 0x00;
pub const EP_TYPE_ISO: u8 = 0x01;
pub const EP_TYPE_BULK: u8 = 0x02;
pub const EP_TYPE_INTERRUPT: u8 = 0x03;

// ─── Standard requests ───────────────────────────────────────────────────────
pub const REQ_GET_DESCRIPTOR: u8 = 0x06;
pub const REQ_SET_CONFIGURATION: u8 = 0x09;
pub const REQ_SET_ADDRESS: u8 = 0x05;
/// HID class request `SET_PROTOCOL` (`HID 1.11` §7.2.5).
pub const REQ_HID_SET_PROTOCOL: u8 = 0x0B;
/// HID class request `SET_IDLE` (`HID 1.11` §7.2.4).
pub const REQ_HID_SET_IDLE: u8 = 0x0A;
pub const REQ_HID_GET_REPORT: u8 = 0x01;

/// `bmRequestType` for a device→host standard request.
pub const RT_DEV_TO_HOST_STANDARD: u8 = 0x80;
/// `bmRequestType` for a host→device class request targeting an interface.
pub const RT_CLASS_INTERFACE_OUT: u8 = 0x21;

/// Parse errors that also serve as "keep walking" signals for the walker.
pub fn le16(buf: &[u8], off: usize) -> Option<u16> {
    let b = buf.get(off..off + 2)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}

/// Parsed device descriptor (USB 2.0 Table 9-8).
#[derive(Clone, Copy, Debug, Default)]
pub struct DeviceDescriptor {
    pub bcd_usb: u16,
    pub class: u8,
    pub subclass: u8,
    pub protocol: u8,
    pub max_packet_size_0: u8,
    pub vendor_id: u16,
    pub product_id: u16,
    pub num_configurations: u8,
}

impl DeviceDescriptor {
    pub fn parse(buf: &[u8]) -> Option<Self> {
        if buf.len() < 18 || buf[1] != DT_DEVICE {
            return None;
        }
        Some(Self {
            bcd_usb: le16(buf, 2)?,
            class: buf[4],
            subclass: buf[5],
            protocol: buf[6],
            max_packet_size_0: buf[7],
            vendor_id: le16(buf, 8)?,
            product_id: le16(buf, 10)?,
            num_configurations: buf[17],
        })
    }
}

/// One interface and everything declared under it.
#[derive(Clone, Debug, Default)]
pub struct InterfaceDesc {
    pub number: u8,
    pub alternate: u8,
    pub class: u8,
    pub subclass: u8,
    pub protocol: u8,
    /// Interrupt IN endpoints declared by this interface.
    pub interrupt_in: Vec<EndpointDesc>,
    /// Byte length of the HID report descriptor, when one is declared.
    pub report_descriptor_len: Option<u16>,
}

impl InterfaceDesc {
    /// True for a HID interface this driver can drive.
    pub fn is_hid(&self) -> bool {
        self.class == CLASS_HID
    }

    /// True when the interface declares it supports boot protocol.
    pub fn supports_boot(&self) -> bool {
        self.subclass == SUBCLASS_BOOT
    }
}

/// Parsed endpoint descriptor (USB 2.0 Table 9-13).
#[derive(Clone, Copy, Debug, Default)]
pub struct EndpointDesc {
    pub address: u8,
    pub attributes: u8,
    pub max_packet_size: u16,
    pub interval: u8,
}

impl EndpointDesc {
    pub fn number(&self) -> u8 {
        self.address & 0x0F
    }

    pub fn is_in(&self) -> bool {
        self.address & 0x80 != 0
    }

    pub fn transfer_type(&self) -> u8 {
        self.attributes & EP_TYPE_MASK
    }
}

/// Walk a configuration descriptor and collect its HID interfaces.
///
/// Non-HID interfaces are skipped but still tracked, so an interface's
/// endpoints are attributed to the right interface even when a device
/// interleaves them (which composite receivers commonly do).
pub fn parse_configuration(buf: &[u8]) -> Vec<InterfaceDesc> {
    let mut interfaces: Vec<InterfaceDesc> = Vec::new();
    let mut i = 0usize;

    while i + 2 <= buf.len() {
        let len = buf[i] as usize;
        let dtype = buf[i + 1];
        // A zero length would spin forever; a length past the end is truncation.
        if len < 2 || i + len > buf.len() {
            break;
        }
        let body = &buf[i..i + len];

        match dtype {
            DT_INTERFACE if len >= 9 => {
                interfaces.push(InterfaceDesc {
                    number: body[2],
                    alternate: body[3],
                    class: body[5],
                    subclass: body[6],
                    protocol: body[7],
                    interrupt_in: Vec::new(),
                    report_descriptor_len: None,
                });
            }
            DT_ENDPOINT if len >= 7 => {
                let ep = EndpointDesc {
                    address: body[2],
                    attributes: body[3],
                    max_packet_size: le16(body, 4).unwrap_or(0) & 0x07FF,
                    interval: body[6],
                };
                if let Some(iface) = interfaces.last_mut() {
                    if ep.is_in() && ep.transfer_type() == EP_TYPE_INTERRUPT {
                        iface.interrupt_in.push(ep);
                    }
                }
            }
            DT_HID if len >= 9 => {
                // bNumDescriptors counts HID class descriptors (report, and
                // optionally a physical descriptor) that follow inline.
                let count = body[5] as usize;
                if let Some(iface) = interfaces.last_mut() {
                    for n in 0..count {
                        let off = 6 + n * 3;
                        if off + 3 > len {
                            break;
                        }
                        if body[off] == DT_HID_REPORT {
                            iface.report_descriptor_len = le16(body, off + 1);
                        }
                    }
                }
            }
            _ => {}
        }

        i += len;
    }

    interfaces
}

/// Total byte length declared by a configuration descriptor's header.
///
/// Read from the first 9 bytes so the caller can then fetch the whole blob in
/// one transfer instead of guessing a buffer size.
pub fn configuration_total_length(buf: &[u8]) -> Option<u16> {
    if buf.len() < 9 || buf[1] != DT_CONFIGURATION {
        return None;
    }
    le16(buf, 2)
}

/// Decode a USB string descriptor (UTF-16LE) into ASCII-ish bytes.
///
/// Returns `None` when the descriptor is malformed. Non-ASCII code units are
/// replaced with `?` — descriptor strings are informational only (device names
/// for logging) so nothing depends on their exact content.
pub fn parse_string_descriptor(buf: &[u8], out: &mut [u8]) -> Option<usize> {
    if buf.len() < 2 || buf[1] != DT_STRING {
        return None;
    }
    let len = buf[0] as usize;
    if len < 2 || len > buf.len() {
        return None;
    }
    let mut n = 0usize;
    let mut i = 2usize;
    while i + 1 < len && n < out.len() {
        let unit = u16::from_le_bytes([buf[i], buf[i + 1]]);
        out[n] = if (0x20..0x7F).contains(&unit) {
            unit as u8
        } else {
            b'?'
        };
        n += 1;
        i += 2;
    }
    Some(n)
}
