//! Keyboard LED output reports.
//!
//! Lock state is the one piece of keyboard state that travels the other way:
//! the input service sees the Caps/Num/Scroll Lock keys, and the keyboard's own
//! LEDs are the only place a user can see what they did. The host writes that
//! state back as an Output report — `SET_REPORT` — and this module turns the
//! service's lock bitmap into the bytes a particular device expects.
//!
//! Two shapes exist and both are in the field:
//!
//! * **Declared.** The report descriptor carries an Output item with LED-page
//!   usages, each entry a one-bit variable. Its report ID, entry order and bit
//!   offsets are the device's own and are used as declared.
//! * **Boot.** `HID 1.11` Appendix B.1 fixes the boot keyboard's LED byte: Num
//!   Lock in bit 0, Caps Lock in bit 1, Scroll Lock in bit 2, no report ID. A
//!   device that came up through the boot fallback, or that declares no LED
//!   output while claiming boot support, takes this one.

extern crate alloc;

use alloc::vec::Vec;

use super::keymap::{LED_CAPS_LOCK, LED_NUM_LOCK, LED_SCROLL_LOCK, PAGE_LED};
use super::report::HidReportMap;

/// Longest LED output report this driver will send.
///
/// Every keyboard in the wild declares one byte, and the boot byte is one; the
/// bound exists so a vendor descriptor cannot make the driver send an unbounded
/// control transfer.
pub const MAX_LED_REPORT: usize = 8;

/// The output report that carries one keyboard's LEDs.
pub struct LedOutput {
    report_id: u8,
    /// Payload length in bytes, from the report's declared bit length.
    len: usize,
    /// `(LED usage, bit offset within the payload)` for every LED the report
    /// declares, in declaration order.
    bits: Vec<(u32, u32)>,
}

impl LedOutput {
    /// Take the LED output report from a parsed descriptor, if it declares one.
    ///
    /// Returns `None` when no Output item names an LED-page usage: a device that
    /// declared none cannot be told about lock state, and guessing a report ID
    /// or bit order at it would put arbitrary bits in a control transfer.
    pub fn from_map(map: &HidReportMap) -> Option<Self> {
        let report = map.outputs.iter().find(|r| {
            r.fields
                .iter()
                .any(|f| (0..f.count).any(|i| f.variable_usage(i).is_some_and(is_led_usage)))
        })?;

        let mut bits = Vec::new();
        for field in report.fields.iter() {
            // An LED bitmap is a variable field — one bit per LED. An array
            // field would mean the value *is* the usage, which is not how any
            // keyboard declares LEDs.
            if !field.is_variable {
                continue;
            }
            for index in 0..field.count {
                let Some(usage) = field.variable_usage(index) else {
                    continue;
                };
                if is_led_usage(usage) {
                    bits.push((
                        usage & 0xFFFF,
                        field.offset_bits.saturating_add(index * field.size_bits),
                    ));
                }
            }
        }
        if bits.is_empty() {
            return None;
        }

        Some(Self {
            report_id: report.report_id,
            len: report
                .total_bits
                .div_ceil(8)
                .clamp(1, MAX_LED_REPORT as u32) as usize,
            bits,
        })
    }

    /// The fixed boot-protocol LED report: one byte, no report ID.
    pub fn boot() -> Self {
        Self {
            report_id: 0,
            len: 1,
            bits: alloc::vec![(LED_NUM_LOCK, 0), (LED_CAPS_LOCK, 1), (LED_SCROLL_LOCK, 2),],
        }
    }

    /// The report ID this device wants in `wValue` (0 when it uses none).
    pub fn report_id(&self) -> u8 {
        self.report_id
    }

    /// Payload length in bytes — what the control transfer carries.
    pub fn payload_len(&self) -> usize {
        self.len
    }

    /// Encode `leds` (an `api::ipc::led_bits` bitmap) into `out`, returning the
    /// payload length to send.
    ///
    /// LEDs the report does not declare are dropped rather than placed by
    /// position: a device that declares only Caps Lock must not receive the Num
    /// Lock bit one entry over.
    pub fn encode(&self, leds: u8, out: &mut [u8; MAX_LED_REPORT]) -> usize {
        out[..self.len].fill(0);
        for &(usage, bit) in self.bits.iter() {
            if !led_is_on(usage, leds) {
                continue;
            }
            let byte = (bit / 8) as usize;
            if byte < self.len {
                out[byte] |= 1 << (bit % 8);
            }
        }
        self.len
    }
}

/// Whether a decoded usage is one of the three keyboard LEDs.
fn is_led_usage(usage: u32) -> bool {
    usage >> 16 == PAGE_LED
        && matches!(
            usage & 0xFFFF,
            LED_NUM_LOCK | LED_CAPS_LOCK | LED_SCROLL_LOCK
        )
}

/// Whether `leds` turns on the LED a usage names.
fn led_is_on(usage: u32, leds: u8) -> bool {
    let bit = match usage & 0xFFFF {
        LED_NUM_LOCK => api::ipc::led_bits::NUM_LOCK,
        LED_CAPS_LOCK => api::ipc::led_bits::CAPS_LOCK,
        LED_SCROLL_LOCK => api::ipc::led_bits::SCROLL_LOCK,
        _ => return false,
    };
    leds & bit != 0
}

#[cfg(test)]
mod tests {
    use super::{LedOutput, MAX_LED_REPORT};
    use crate::hid::parse_report_descriptor;
    use api::ipc::led_bits;

    /// A keyboard that declares the boot LED byte: five one-bit LEDs then three
    /// padding bits, no report IDs, alongside a boot-shaped input report.
    fn boot_led_descriptor() -> &'static [u8] {
        &[
            0x05, 0x01, // Usage Page (Generic Desktop)
            0x09, 0x06, // Usage (Keyboard)
            0xA1, 0x01, // Collection (Application)
            0x05, 0x08, //   Usage Page (LED)
            0x19, 0x01, //   Usage Minimum (Num Lock)
            0x29, 0x05, //   Usage Maximum (Kana)
            0x95, 0x05, //   Report Count (5)
            0x75, 0x01, //   Report Size (1)
            0x91, 0x02, //   Output (Data, Variable, Absolute)
            0x95, 0x01, //   Report Count (1)
            0x75, 0x03, //   Report Size (3)
            0x91, 0x03, //   Output (Constant)
            0x05, 0x07, //   Usage Page (Keyboard)
            0x19, 0xE0, //   Usage Minimum (Left Control)
            0x29, 0xE7, //   Usage Maximum (Right GUI)
            0x95, 0x08, //   Report Count (8)
            0x75, 0x01, //   Report Size (1)
            0x81, 0x02, //   Input (Data, Variable, Absolute)
            0x95, 0x06, //   Report Count (6)
            0x75, 0x08, //   Report Size (8)
            0x81, 0x00, //   Input (Data, Array)
            0xC0, // End Collection
        ]
    }

    /// A device that declares its LEDs out of order, under report ID 2, with the
    /// input report under a different ID.
    fn reordered_report_id_descriptor() -> &'static [u8] {
        &[
            0x05, 0x01, // Usage Page (Generic Desktop)
            0x09, 0x06, // Usage (Keyboard)
            0xA1, 0x01, // Collection (Application)
            0x85, 0x02, //   Report ID (2)
            0x05, 0x08, //   Usage Page (LED)
            0x09, 0x02, //   Usage (Caps Lock)
            0x09, 0x01, //   Usage (Num Lock)
            0x09, 0x03, //   Usage (Scroll Lock)
            0x95, 0x03, //   Report Count (3)
            0x75, 0x01, //   Report Size (1)
            0x91, 0x02, //   Output (Data, Variable, Absolute)
            0x95, 0x05, //   Report Count (5)
            0x75, 0x01, //   Report Size (1)
            0x91, 0x03, //   Output (Constant)
            0x85, 0x01, //   Report ID (1)
            0x05, 0x07, //   Usage Page (Keyboard)
            0x19, 0xE0, //   Usage Minimum (Left Control)
            0x29, 0xE7, //   Usage Maximum (Right GUI)
            0x95, 0x08, //   Report Count (8)
            0x75, 0x01, //   Report Size (1)
            0x81, 0x02, //   Input (Data, Variable, Absolute)
            0x95, 0x06, //   Report Count (6)
            0x75, 0x08, //   Report Size (8)
            0x81, 0x00, //   Input (Data, Array)
            0xC0, // End Collection
        ]
    }

    #[test]
    fn boot_layout_places_the_three_lock_leds_in_the_boot_byte() {
        let map = parse_report_descriptor(boot_led_descriptor());
        let output = LedOutput::from_map(&map).expect("LED output report");

        assert_eq!(output.report_id(), 0);
        assert_eq!(output.payload_len(), 1);

        let mut buf = [0u8; MAX_LED_REPORT];
        assert_eq!(output.encode(0, &mut buf), 1);
        assert_eq!(buf[0], 0);

        assert_eq!(output.encode(led_bits::CAPS_LOCK, &mut buf), 1);
        assert_eq!(buf[0], 0b010);

        let all = led_bits::NUM_LOCK | led_bits::CAPS_LOCK | led_bits::SCROLL_LOCK;
        assert_eq!(output.encode(all, &mut buf), 1);
        assert_eq!(buf[0], 0b111);
    }

    #[test]
    fn led_bits_follow_the_declared_usages_not_their_order() {
        let map = parse_report_descriptor(reordered_report_id_descriptor());
        let output = LedOutput::from_map(&map).expect("LED output report");

        assert_eq!(output.report_id(), 2);
        assert_eq!(output.payload_len(), 1);

        // The device's input report is still its own: routing Output items into
        // a separate table must not move anything the decoder reads.
        assert!(map.layout(1).is_some_and(|r| !r.fields.is_empty()));

        let mut buf = [0u8; MAX_LED_REPORT];
        // Caps Lock is declared first here, so it must land in bit 0 and Num
        // Lock in bit 1 — a device's report is its own, not the boot byte's.
        assert_eq!(output.encode(led_bits::CAPS_LOCK, &mut buf), 1);
        assert_eq!(buf[0], 0b001);
        assert_eq!(output.encode(led_bits::NUM_LOCK, &mut buf), 1);
        assert_eq!(buf[0], 0b010);
        assert_eq!(output.encode(led_bits::SCROLL_LOCK, &mut buf), 1);
        assert_eq!(buf[0], 0b100);
    }

    #[test]
    fn a_device_without_an_led_output_report_gets_none() {
        // The same boot keyboard with its LED output items removed: nothing
        // declares where a lock bit would go, so no report may be invented.
        let stripped: &[u8] = &[
            0x05, 0x01, 0x09, 0x06, 0xA1, 0x01, 0x05, 0x07, 0x19, 0xE0, 0x29, 0xE7, 0x95, 0x08,
            0x75, 0x01, 0x81, 0x02, 0x95, 0x06, 0x75, 0x08, 0x81, 0x00, 0xC0,
        ];
        let map = parse_report_descriptor(stripped);
        assert!(!map.reports.is_empty());
        assert!(LedOutput::from_map(&map).is_none());

        // A descriptor with a non-LED output report is not an LED report either.
        let vendor_output: &[u8] = &[
            0x06, 0x00, 0xFF, // Usage Page (Vendor Defined)
            0x09, 0x01, // Usage (Vendor 1)
            0xA1, 0x01, // Collection (Application)
            0x09, 0x02, //   Usage (Vendor 2)
            0x95, 0x08, //   Report Count (8)
            0x75, 0x01, //   Report Size (1)
            0x91, 0x02, //   Output (Data, Variable, Absolute)
            0xC0,
        ];
        assert!(LedOutput::from_map(&parse_report_descriptor(vendor_output)).is_none());
    }

    #[test]
    fn boot_fallback_is_the_fixed_boot_byte() {
        let output = LedOutput::boot();
        assert_eq!(output.report_id(), 0);
        assert_eq!(output.payload_len(), 1);

        let mut buf = [0u8; MAX_LED_REPORT];
        assert_eq!(output.encode(led_bits::NUM_LOCK, &mut buf), 1);
        assert_eq!(buf[0], 0b001);
    }
}
