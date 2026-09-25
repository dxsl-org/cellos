//! HID input: report-descriptor parsing, report decoding, and evdev translation.
//!
//! The pipeline is:
//!
//! ```text
//!   GET_DESCRIPTOR(REPORT)  -> report::parse_report_descriptor  -> HidReportMap
//!   Interrupt IN payload    -> decode::decode_auto              -> [HidValue]
//!   [HidValue]              -> HidDecoder                       -> [EvdevEvent]
//!   [EvdevEvent]            -> EvdevEvent::encode               -> input-service bytes
//! ```
//!
//! A HID device mixes two field kinds and both must be handled, because which
//! one a key uses is a descriptor author's choice:
//!
//! * **Array** fields carry the usage itself — a keyboard's six key slots, so a
//!   key's *absence* is its release and the decoder derives edges by diffing.
//! * **Variable** fields carry a value per entry — modifier bitmaps, mouse
//!   button bits, and relative axes. Here a release *is* reported (value 0).
//!
//! The two kinds keep separate held-state so an array diff can never emit a
//! release for a key that only ever appears as a variable field.

pub mod decode;
pub mod keymap;
pub mod leds;
pub mod report;

extern crate alloc;

use alloc::vec::Vec;

pub use decode::{decode_auto, decode_report, HidValue};
pub use leds::{LedOutput, MAX_LED_REPORT};
pub use report::{parse_report_descriptor, HidCollection, HidReportMap};

// ─── Input-service wire opcodes (must match cells/services/input) ─────────────
/// `EV_KEY` — a key or mouse button changed state.
pub const EV_KEY: u8 = 0;
/// `EV_REL` — a relative pointer axis moved.
pub const EV_REL: u8 = 1;
/// `EV_ABS` — an absolute pointer position (unused by this driver).
pub const EV_ABS: u8 = 2;
/// A HID event with a stable logical-device identity.
pub const EV_DEVICE: u8 = 0x05;

/// Host-local logical HID interface identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HidDeviceId(pub u32);

/// Length of a device-identified input event.
///
/// Layout: `[EV_DEVICE][device:u32][event_type][code:u32][value:i32]`.
pub const DEVICE_EVENT_LEN: usize = 14;

/// Serialise an event from one logical HID interface.
pub fn encode_device_event(
    device: HidDeviceId,
    opcode: u8,
    code: u32,
    value: i32,
    buf: &mut [u8; DEVICE_EVENT_LEN],
) {
    buf[0] = EV_DEVICE;
    buf[1..5].copy_from_slice(&device.0.to_le_bytes());
    buf[5] = opcode;
    buf[6..10].copy_from_slice(&code.to_le_bytes());
    buf[10..14].copy_from_slice(&value.to_le_bytes());
}

/// One decoded event, in evdev terms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvdevEvent {
    Key { code: u32, pressed: bool },
    Rel { code: u32, value: i32 },
}

impl EvdevEvent {
    /// Serialise an event with its originating HID logical-interface identity.
    pub fn encode_device(&self, device: HidDeviceId, buf: &mut [u8; DEVICE_EVENT_LEN]) {
        match *self {
            EvdevEvent::Key { code, pressed } => {
                encode_device_event(device, EV_KEY, code, if pressed { 1 } else { 0 }, buf)
            }
            EvdevEvent::Rel { code, value } => {
                encode_device_event(device, EV_REL, code, value, buf)
            }
        }
    }
}

/// Which device class a HID interface serves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HidKind {
    Keyboard,
    Mouse,
    /// A combo dongle that packs both into one interface.
    Combo,
    Unknown,
}

impl HidKind {
    /// Classify from the parsed report map's collections.
    pub fn from_map(map: &HidReportMap) -> Self {
        let k = map.has_collection(HidCollection::Keyboard);
        let m = map.has_collection(HidCollection::Mouse);
        match (k, m) {
            (true, true) => HidKind::Combo,
            (true, false) => HidKind::Keyboard,
            (false, true) => HidKind::Mouse,
            (false, false) => HidKind::Unknown,
        }
    }
}

/// Stateful decoder holding the previous report so edges can be derived.
pub struct HidDecoder {
    map: HidReportMap,
    /// evdev codes currently down from **Array** fields (keyboard key slots).
    held_array_keys: Vec<u32>,
    /// `(evdev code, pressed)` from **Variable** fields — modifiers, buttons.
    held_variable: Vec<(u32, bool)>,
    /// Relative axes are noisy at rest; suppress explicit zero deltas.
    suppress_zero_rel: bool,
}

impl HidDecoder {
    pub fn new(map: HidReportMap) -> Self {
        Self {
            map,
            held_array_keys: Vec::new(),
            held_variable: Vec::new(),
            suppress_zero_rel: true,
        }
    }

    pub fn map(&self) -> &HidReportMap {
        &self.map
    }

    pub fn kind(&self) -> HidKind {
        HidKind::from_map(&self.map)
    }

    /// Feed one raw report and receive the events it implies.
    pub fn process(&mut self, report: &[u8], out: &mut Vec<EvdevEvent>) {
        let values = decode_auto(&self.map, report);

        self.process_array_keys(&values, out);
        self.process_variable_fields(&values, out);
    }

    /// Array fields: the report lists keys currently down, so diff the set.
    fn process_array_keys(&mut self, values: &[HidValue], out: &mut Vec<EvdevEvent>) {
        let mut now_held: Vec<u32> = Vec::new();
        for v in values.iter().filter(|v| !v.is_variable) {
            if v.page != keymap::PAGE_KEYBOARD {
                continue;
            }
            if let Some(code) = keymap::keyboard_to_evdev(v.usage) {
                if !now_held.contains(&code) {
                    now_held.push(code);
                }
            }
        }

        for code in &now_held {
            if !self.held_array_keys.contains(code) {
                out.push(EvdevEvent::Key {
                    code: *code,
                    pressed: true,
                });
            }
        }
        for code in &self.held_array_keys {
            if !now_held.contains(code) {
                out.push(EvdevEvent::Key {
                    code: *code,
                    pressed: false,
                });
            }
        }
        self.held_array_keys = now_held;
    }

    /// Variable fields: every entry reports its own value, including releases.
    fn process_variable_fields(&mut self, values: &[HidValue], out: &mut Vec<EvdevEvent>) {
        for v in values.iter().filter(|v| v.is_variable) {
            // Relative axes move the pointer; nothing else uses value semantics.
            if v.page == keymap::PAGE_GENERIC_DESKTOP && v.collection == HidCollection::Mouse {
                if let Some(rel) = keymap::generic_desktop_to_rel(v.usage) {
                    if v.value != 0 || !self.suppress_zero_rel {
                        out.push(EvdevEvent::Rel {
                            code: rel,
                            value: v.value,
                        });
                    }
                }
                continue;
            }

            // Modifier bitmaps and mouse buttons are edge-producing.
            let Some((code, _is_button)) = keymap::hid_field_to_evdev(v.page, v.usage) else {
                continue;
            };
            let pressed = v.value != 0;
            let prev = self
                .held_variable
                .iter()
                .find(|(c, _)| *c == code)
                .map(|(_, p)| *p);
            if prev == Some(pressed) {
                continue;
            }
            match self.held_variable.iter_mut().find(|(c, _)| *c == code) {
                Some(entry) => entry.1 = pressed,
                None => self.held_variable.push((code, pressed)),
            }
            out.push(EvdevEvent::Key { code, pressed });
        }
    }
}

/// Held state for the boot-protocol fallback path.
///
/// Kept separate per device class so a keyboard report's diff cannot release a
/// mouse button (they travel in different report shapes but share this decoder).
#[derive(Default)]
pub struct BootState {
    keys: Vec<u32>,
    buttons: Vec<u32>,
}

/// Decode a HID **boot protocol** report without any descriptor.
///
/// Boot protocol is the fixed layout a device must also support so a BIOS can
/// use it (`HID 1.11` Appendix B): keyboards send 8 bytes — modifier bitmap,
/// reserved, then six usage slots — and mice send 3 (or 4 with a wheel).
///
/// Used as the fallback when a device's report descriptor is missing or fails
/// to parse. A combo receiver usually implements it for both halves, which is
/// exactly why it is worth keeping alongside the descriptor path.
pub fn decode_boot_report(
    kind: HidKind,
    report: &[u8],
    state: &mut BootState,
    out: &mut Vec<EvdevEvent>,
) {
    match kind {
        HidKind::Keyboard | HidKind::Combo if report.len() >= 8 => {
            let mut now_held: Vec<u32> = Vec::new();
            for code in keymap::boot_modifiers(report[0]) {
                if !now_held.contains(&code) {
                    now_held.push(code);
                }
            }
            for &usage in &report[2..8] {
                if usage == 0 {
                    continue;
                }
                if let Some(code) = keymap::keyboard_to_evdev(usage as u32) {
                    if !now_held.contains(&code) {
                        now_held.push(code);
                    }
                }
            }
            for code in &now_held {
                if !state.keys.contains(code) {
                    out.push(EvdevEvent::Key {
                        code: *code,
                        pressed: true,
                    });
                }
            }
            for code in &state.keys {
                if !now_held.contains(code) {
                    out.push(EvdevEvent::Key {
                        code: *code,
                        pressed: false,
                    });
                }
            }
            state.keys = now_held;
        }
        HidKind::Mouse if report.len() >= 3 => {
            let buttons = report[0];
            for (bit, code) in [
                keymap::BTN_LEFT,
                keymap::BTN_RIGHT,
                keymap::BTN_MIDDLE,
                keymap::BTN_SIDE,
                keymap::BTN_EXTRA,
            ]
            .iter()
            .enumerate()
            {
                let pressed = buttons & (1 << bit) != 0;
                let prev = state.buttons.contains(code);
                if pressed == prev {
                    continue;
                }
                if pressed {
                    state.buttons.push(*code);
                } else {
                    state.buttons.retain(|c| c != code);
                }
                out.push(EvdevEvent::Key {
                    code: *code,
                    pressed,
                });
            }
            // Boot mouse deltas are signed 8-bit.
            let dx = report[1] as i8 as i32;
            let dy = report[2] as i8 as i32;
            if dx != 0 {
                out.push(EvdevEvent::Rel {
                    code: keymap::REL_X,
                    value: dx,
                });
            }
            if dy != 0 {
                out.push(EvdevEvent::Rel {
                    code: keymap::REL_Y,
                    value: dy,
                });
            }
            if report.len() >= 4 {
                let wheel = report[3] as i8 as i32;
                if wheel != 0 {
                    out.push(EvdevEvent::Rel {
                        code: keymap::REL_WHEEL,
                        value: wheel,
                    });
                }
            }
        }
        _ => {}
    }
}
