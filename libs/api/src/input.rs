// SPDX-License-Identifier: MPL-2.0

//! Input event types for ViCell cells.
//!
//! `InputEvent` is the canonical event type exchanged between the kernel-side
//! VirtIO input driver and any consumer cell (shell, compositor, …).  The input
//! service cell (`cells/services/input`) translates raw scancodes into these
//! types before dispatching to the focused cell.

// ─── Key state ───────────────────────────────────────────────────────────────

/// Whether a key or button was pressed, released, or held.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    Released = 0,
    Pressed  = 1,
    /// Key auto-repeat fired by the keyboard hardware.
    Repeated = 2,
}

// ─── Modifier flags ───────────────────────────────────────────────────────────

/// Bitmask of modifier keys active at the time of a `KeyEvent`.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers(pub u8);

impl Modifiers {
    pub const SHIFT:       Modifiers = Modifiers(0b0000_0001);
    pub const CTRL:        Modifiers = Modifiers(0b0000_0010);
    pub const ALT:         Modifiers = Modifiers(0b0000_0100);
    pub const META:        Modifiers = Modifiers(0b0000_1000);
    pub const CAPS_LOCK:   Modifiers = Modifiers(0b0001_0000);
    pub const NUM_LOCK:    Modifiers = Modifiers(0b0010_0000);
    pub const SCROLL_LOCK: Modifiers = Modifiers(0b0100_0000);

    pub fn contains(self, other: Modifiers) -> bool {
        (self.0 & other.0) == other.0
    }

    pub fn set(&mut self, other: Modifiers) {
        self.0 |= other.0;
    }

    pub fn clear(&mut self, other: Modifiers) {
        self.0 &= !other.0;
    }

    pub fn toggle(&mut self, other: Modifiers) {
        self.0 ^= other.0;
    }
}

// ─── KeySym ──────────────────────────────────────────────────────────────────

/// Virtual key identifier — layout-independent.
///
/// Printable characters are represented by their Unicode code point in the
/// range 0x0020..=0xFFFF.  Special keys use the negative range or values
/// above 0x1_0000.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySym {
    Unknown    = 0,
    // Control keys
    Escape     = 0x0001,
    Return     = 0x0002,
    Backspace  = 0x0003,
    Tab        = 0x0004,
    Delete     = 0x0005,
    Insert     = 0x0006,
    // Arrow keys
    Up         = 0x0010,
    Down       = 0x0011,
    Left       = 0x0012,
    Right      = 0x0013,
    // Page navigation
    Home       = 0x0020,
    End        = 0x0021,
    PageUp     = 0x0022,
    PageDown   = 0x0023,
    // Function keys
    F1         = 0x0101,
    F2         = 0x0102,
    F3         = 0x0103,
    F4         = 0x0104,
    F5         = 0x0105,
    F6         = 0x0106,
    F7         = 0x0107,
    F8         = 0x0108,
    F9         = 0x0109,
    F10        = 0x010A,
    F11        = 0x010B,
    F12        = 0x010C,
    // Printable (the u32 value = Unicode code point)
    Printable  = 0x8000,
}

// ─── Key event ───────────────────────────────────────────────────────────────

/// A single keyboard event with full modifier and character context.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct KeyEvent {
    /// Monotonic tick timestamp from the kernel (10 MHz on QEMU RV64).
    pub timestamp_ticks: u64,
    /// Raw EV_KEY scancode from the VirtIO input device.
    pub scancode: u32,
    /// Virtual key identifier (layout-independent).
    pub keysym: KeySym,
    /// Unicode character for printable keys, 0 otherwise.
    pub character: u32,
    /// Active modifier flags at the time of the event.
    pub modifiers: Modifiers,
    /// Whether the key was pressed, released, or repeated.
    pub state: KeyState,
    pub _pad: [u8; 2],
}

impl KeyEvent {
    /// Return `Some(char)` if the event carries a printable Unicode character.
    pub fn char(&self) -> Option<char> {
        if self.character == 0 { return None; }
        char::from_u32(self.character)
    }

    /// Return true if Ctrl+<letter> was pressed (e.g. Ctrl+C → true for 'C').
    pub fn is_ctrl(&self, letter: char) -> bool {
        self.modifiers.contains(Modifiers::CTRL)
            && self.char().map(|c| c.eq_ignore_ascii_case(&letter)).unwrap_or(false)
    }
}

// ─── Mouse events ────────────────────────────────────────────────────────────

/// A mouse button identifier.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton { Left = 0, Right = 1, Middle = 2, Back = 3, Forward = 4 }

// ─── Top-level event ─────────────────────────────────────────────────────────

/// Union of all input event variants dispatched by the input service cell.
#[repr(C, u8)]
#[derive(Debug, Clone, Copy)]
pub enum InputEvent {
    Key(KeyEvent),
    MouseMove { x: i32, y: i32, dx: i32, dy: i32 },
    MouseButton { button: MouseButton, state: KeyState },
    MouseScroll { dx: i32, dy: i32 },
}

// ─── IPC wire format ─────────────────────────────────────────────────────────

/// Maximum serialised size of an `InputEvent` over IPC (64 bytes reserved).
pub const INPUT_EVENT_IPC_SIZE: usize = 64;

/// Opcode byte that prefixes every input-service → app IPC message.
///
/// Layout: `[INPUT_EVENT_OPCODE: u8][encode_event output: 64 bytes]`
pub const INPUT_EVENT_OPCODE: u8 = 0x10;

/// Deserialise a 64-byte `encode_event` buffer back into an `InputEvent`.
///
/// Returns `None` for unknown discriminants or truncated payloads.
pub fn decode_event(buf: &[u8]) -> Option<InputEvent> {
    if buf.is_empty() { return None; }
    match buf[0] {
        0 => { // Key
            let p = buf.get(1..)?;
            if p.len() < 22 { return None; }
            let ts  = u64::from_le_bytes([p[0],p[1],p[2],p[3],p[4],p[5],p[6],p[7]]);
            let sc  = u32::from_le_bytes([p[8],p[9],p[10],p[11]]);
            let ksv = u32::from_le_bytes([p[12],p[13],p[14],p[15]]);
            let ch  = u32::from_le_bytes([p[16],p[17],p[18],p[19]]);
            let mods = Modifiers(p[20]);
            let state = match p[21] {
                0 => KeyState::Released,
                1 => KeyState::Pressed,
                2 => KeyState::Repeated,
                _ => return None,
            };
            let keysym = keysym_from_u32(ksv);
            Some(InputEvent::Key(KeyEvent {
                timestamp_ticks: ts,
                scancode: sc,
                keysym,
                character: ch,
                modifiers: mods,
                state,
                _pad: [0; 2],
            }))
        }
        1 => { // MouseMove
            let p = buf.get(1..)?;
            if p.len() < 16 { return None; }
            let x  = i32::from_le_bytes([p[0],p[1],p[2],p[3]]);
            let y  = i32::from_le_bytes([p[4],p[5],p[6],p[7]]);
            let dx = i32::from_le_bytes([p[8],p[9],p[10],p[11]]);
            let dy = i32::from_le_bytes([p[12],p[13],p[14],p[15]]);
            Some(InputEvent::MouseMove { x, y, dx, dy })
        }
        2 => { // MouseButton
            let p = buf.get(1..)?;
            if p.len() < 2 { return None; }
            let button = match p[0] {
                0 => MouseButton::Left,
                1 => MouseButton::Right,
                2 => MouseButton::Middle,
                3 => MouseButton::Back,
                4 => MouseButton::Forward,
                _ => return None,
            };
            let state = match p[1] {
                0 => KeyState::Released,
                1 => KeyState::Pressed,
                _ => return None,
            };
            Some(InputEvent::MouseButton { button, state })
        }
        3 => { // MouseScroll
            let p = buf.get(1..)?;
            if p.len() < 8 { return None; }
            let dx = i32::from_le_bytes([p[0],p[1],p[2],p[3]]);
            let dy = i32::from_le_bytes([p[4],p[5],p[6],p[7]]);
            Some(InputEvent::MouseScroll { dx, dy })
        }
        _ => None,
    }
}

/// Map a `repr(u32)` KeySym discriminant back to the enum variant.
fn keysym_from_u32(v: u32) -> KeySym {
    match v {
        0x0001 => KeySym::Escape,
        0x0002 => KeySym::Return,
        0x0003 => KeySym::Backspace,
        0x0004 => KeySym::Tab,
        0x0005 => KeySym::Delete,
        0x0006 => KeySym::Insert,
        0x0010 => KeySym::Up,
        0x0011 => KeySym::Down,
        0x0012 => KeySym::Left,
        0x0013 => KeySym::Right,
        0x0020 => KeySym::Home,
        0x0021 => KeySym::End,
        0x0022 => KeySym::PageUp,
        0x0023 => KeySym::PageDown,
        0x0101 => KeySym::F1,
        0x0102 => KeySym::F2,
        0x0103 => KeySym::F3,
        0x0104 => KeySym::F4,
        0x0105 => KeySym::F5,
        0x0106 => KeySym::F6,
        0x0107 => KeySym::F7,
        0x0108 => KeySym::F8,
        0x0109 => KeySym::F9,
        0x010A => KeySym::F10,
        0x010B => KeySym::F11,
        0x010C => KeySym::F12,
        0x8000 => KeySym::Printable,
        _      => KeySym::Unknown,
    }
}

/// Serialise an `InputEvent` into a fixed 64-byte IPC buffer.
///
/// Format: byte[0] = discriminant, byte[1..] = variant payload.
pub fn encode_event(ev: &InputEvent, buf: &mut [u8; INPUT_EVENT_IPC_SIZE]) {
    buf.fill(0);
    match ev {
        InputEvent::Key(k) => {
            buf[0] = 0;
            buf[1..9].copy_from_slice(&k.timestamp_ticks.to_le_bytes());
            buf[9..13].copy_from_slice(&k.scancode.to_le_bytes());
            buf[13..17].copy_from_slice(&(k.keysym as u32).to_le_bytes());
            buf[17..21].copy_from_slice(&k.character.to_le_bytes());
            buf[21] = k.modifiers.0;
            buf[22] = k.state as u8;
        }
        InputEvent::MouseMove { x, y, dx, dy } => {
            buf[0] = 1;
            buf[1..5].copy_from_slice(&x.to_le_bytes());
            buf[5..9].copy_from_slice(&y.to_le_bytes());
            buf[9..13].copy_from_slice(&dx.to_le_bytes());
            buf[13..17].copy_from_slice(&dy.to_le_bytes());
        }
        InputEvent::MouseButton { button, state } => {
            buf[0] = 2;
            buf[1] = *button as u8;
            buf[2] = *state as u8;
        }
        InputEvent::MouseScroll { dx, dy } => {
            buf[0] = 3;
            buf[1..5].copy_from_slice(&dx.to_le_bytes());
            buf[5..9].copy_from_slice(&dy.to_le_bytes());
        }
    }
}
