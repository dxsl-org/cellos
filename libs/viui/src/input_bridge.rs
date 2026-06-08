// SPDX-License-Identifier: MIT
//! Converts raw input-service IPC bytes to `viui::Event`.
//!
//! # IPC Format (65-byte message from the input service dispatcher)
//! ```text
//! byte[0]    = INPUT_EVENT_OPCODE (0x10)
//! byte[1]    = InputEvent discriminant: 0=Key 1=MouseMove 2=MouseButton 3=MouseScroll
//! byte[2..]  = event payload (encode_event output, shifted by one opcode byte)
//! ```
//!
//! Key payload (discriminant=0):
//! ```text
//! byte[2..10]  = timestamp_ticks (u64 LE, ignored)
//! byte[10..14] = scancode        (u32 LE, ignored)
//! byte[14..18] = keysym          (u32 LE, api::input::KeySym discriminant)
//! byte[18..22] = character       (u32 LE, Unicode codepoint; 0 = non-printable)
//! byte[22]     = modifiers       (api::input::Modifiers bitmask)
//! byte[23]     = state           (0=Released 1=Pressed 2=Repeated per api::input::KeyState)
//! ```
//!
//! # KeySym values (from api::input::KeySym)
//! Escape=0x01, Return=0x02, Backspace=0x03, Tab=0x04, Delete=0x05, Insert=0x06
//! Up=0x10, Down=0x11, Left=0x12, Right=0x13
//! Home=0x20, End=0x21, PageUp=0x22, PageDown=0x23
//! F1..F12 = 0x0101..0x010C
//! Printable=0x8000 (character field carries the codepoint)
//!
//! # Modifier bitmask (api::input::Modifiers)
//! bit 0 = Shift (0b0000_0001)
//! bit 1 = Ctrl  (0b0000_0010)
//! bit 2 = Alt   (0b0000_0100)
//!
//! # Note on key repeat
//! The input service sends `state=Repeated (2)` for hardware auto-repeat events.
//! `parse_input_message` treats Repeated the same as Pressed so that viui widgets
//! and text-edit fields respond to hardware repeat naturally. The `KeyRepeatState`
//! in `ViApp` provides *software* repeat as a fallback for platforms that do not
//! send hardware repeat events (e.g. touch keyboards, serial consoles).

extern crate alloc;
use alloc::vec::Vec;

use crate::event::{Event, KeyCode, Modifiers, MouseButton};
use crate::layout::Point;

// ─── Opcode ──────────────────────────────────────────────────────────────────

/// Byte[0] of every input-service IPC message.
const INPUT_EVENT_OPCODE: u8 = 0x10;

// ─── KeyState constants (api::input::KeyState repr) ──────────────────────────

const STATE_RELEASED: u8 = 0;
const STATE_PRESSED:  u8 = 1;
/// Hardware auto-repeat — treated identically to Pressed by parse_key_event.
/// Declared here as documentation; the match arm `STATE_RELEASED => drop` implicitly
/// passes both PRESSED and REPEATED through the same branch.
#[allow(dead_code)]
const STATE_REPEATED: u8 = 2;

// ─── KeySym discriminants (api::input::KeySym repr u32) ──────────────────────
// These are the repr(u32) values as returned by `keysym as u32` in encode_event.

const KEYSYM_ESCAPE:    u32 = 0x0001;
const KEYSYM_RETURN:    u32 = 0x0002;
const KEYSYM_BACKSPACE: u32 = 0x0003;
const KEYSYM_TAB:       u32 = 0x0004;
const KEYSYM_DELETE:    u32 = 0x0005;
// INSERT = 0x0006, not in viui::KeyCode — silently dropped
const KEYSYM_UP:        u32 = 0x0010;
const KEYSYM_DOWN:      u32 = 0x0011;
const KEYSYM_LEFT:      u32 = 0x0012;
const KEYSYM_RIGHT:     u32 = 0x0013;
const KEYSYM_HOME:      u32 = 0x0020;
const KEYSYM_END:       u32 = 0x0021;
const KEYSYM_PAGE_UP:   u32 = 0x0022;
const KEYSYM_PAGE_DOWN: u32 = 0x0023;
// F1..F12 = 0x0101..0x010C
const KEYSYM_F_BASE:    u32 = 0x0100; // F1 = 0x0101, Fn = 0x0100 + n
const KEYSYM_F_MAX:     u32 = 0x010C; // F12
// Printable codepoint: keysym == 0x8000; actual char in `character` field
const KEYSYM_PRINTABLE: u32 = 0x8000;

// ─── Modifier bits (api::input::Modifiers) ───────────────────────────────────

const MOD_SHIFT: u8 = 0b0000_0001;
const MOD_CTRL:  u8 = 0b0000_0010;
const MOD_ALT:   u8 = 0b0000_0100;

// ─── Public API ──────────────────────────────────────────────────────────────

/// Parse a single 65-byte input-service IPC message into zero or more `viui::Event`s.
///
/// Returns an empty Vec for:
/// - Messages that are too short, have a wrong opcode, or have unknown discriminants.
/// - Key-Release events (viui does not dispatch `KeyRelease` to widgets yet — add
///   if needed when implementing focus-based key-up routing).
///
/// A Pressed or Repeated key with a printable character emits BOTH `KeyPress` AND
/// `Char(c)` so text-edit widgets and key-shortcut handlers both fire correctly.
pub fn parse_input_message(buf: &[u8]) -> Vec<Event> {
    if buf.len() < 2 || buf[0] != INPUT_EVENT_OPCODE {
        return Vec::new();
    }
    let disc    = buf[1];
    let payload = if buf.len() > 2 { &buf[2..] } else { &[] };

    match disc {
        0 => parse_key_event(payload),
        1 => parse_mouse_move(payload),
        2 => parse_mouse_button(payload),
        3 => parse_mouse_scroll(payload),
        _ => Vec::new(),
    }
}

/// Drain pending input events from the input service (non-blocking).
///
/// Calls `sys_try_recv` in a loop until the queue is empty or `max_events` is reached.
/// Returns events ready for `ViApp::tick_with_dt`. Typically called once per frame before tick.
///
/// If the input service TID is not yet registered (boot race), returns an empty Vec.
pub fn collect_input_events(max_events: usize) -> Vec<Event> {
    use ostd::syscall::{sys_lookup_service, sys_try_recv, SyscallResult};
    use api::syscall::service;

    let input_tid = match sys_lookup_service(service::INPUT) {
        Some(tid) => tid,
        None => return Vec::new(),
    };

    let cap = usize::MAX; // accept messages from any sender (the input service)
    let mut events = Vec::with_capacity(max_events.min(16));

    while events.len() < max_events {
        // buf[0] = 0x10 opcode, buf[1..65] = encode_event payload (64 bytes)
        let mut buf = [0u8; 65];
        match sys_try_recv(cap, &mut buf) {
            // sender_id == 0 → queue empty
            SyscallResult::Ok(0) => break,
            // sender_id > 0 → message received; only process if from input service
            SyscallResult::Ok(sender) if sender == input_tid => {
                let mut parsed = parse_input_message(&buf);
                events.append(&mut parsed);
            }
            // Message from an unexpected sender — discard silently
            SyscallResult::Ok(_) => {}
            SyscallResult::Err(_) => break,
        }
    }

    events
}

// ─── Private parsers ─────────────────────────────────────────────────────────

/// Parse a Key payload (payload starts after the 2-byte opcode+discriminant header).
///
/// Payload layout (indices into `payload` slice, which starts at buf[2]):
/// [0..8]  = timestamp_ticks (u64 LE, ignored)
/// [8..12] = scancode        (u32 LE, ignored)
/// [12..16]= keysym          (u32 LE)
/// [16..20]= character       (u32 LE Unicode codepoint)
/// [20]    = modifiers       (u8 bitmask)
/// [21]    = state           (u8: 0=Released 1=Pressed 2=Repeated)
fn parse_key_event(payload: &[u8]) -> Vec<Event> {
    // Minimum: we need at least 22 bytes for state field
    if payload.len() < 22 {
        return Vec::new();
    }

    let state = payload[21];
    // Drop release events — viui doesn't route KeyRelease to widgets in G1.
    // KeyRelease tracking happens in ViApp::KeyRepeatState for software repeat.
    if state == STATE_RELEASED {
        return Vec::new();
    }

    let keysym    = u32::from_le_bytes([payload[12], payload[13], payload[14], payload[15]]);
    let char_code = u32::from_le_bytes([payload[16], payload[17], payload[18], payload[19]]);
    let mods_byte = payload[20];
    let mods      = decode_modifiers(mods_byte);

    let mut events = Vec::with_capacity(2);

    // Emit KeyPress for non-printable navigation/control keys, and for any
    // key (even printable) so shortcut handlers (Ctrl+S etc.) always fire.
    if let Some(key) = keysym_to_keycode(keysym) {
        events.push(Event::KeyPress { key, modifiers: mods });
    }

    // Emit Char for printable characters (Pressed or Repeated).
    // Skip if Ctrl or Alt is held — those are shortcuts, not text input.
    // (Shift is intentional text — a capital letter is still a character.)
    if char_code != 0
        && !mods.ctrl
        && !mods.alt
    {
        if let Some(c) = char::from_u32(char_code) {
            events.push(Event::Char(c));
        }
    }

    events
}

/// Parse a MouseMove payload.
///
/// Payload layout:
/// [0..4]  = x  (i32 LE, absolute screen position)
/// [4..8]  = y  (i32 LE, absolute screen position)
/// [8..12] = dx (i32 LE, ignored — use absolute position)
/// [12..16]= dy (i32 LE, ignored)
fn parse_mouse_move(payload: &[u8]) -> Vec<Event> {
    if payload.len() < 8 {
        return Vec::new();
    }
    let x = i32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
    let y = i32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
    // Clamp to non-negative screen coordinates
    let pos = Point::new(x.max(0) as f32, y.max(0) as f32);
    alloc::vec![Event::MouseMove { pos }]
}

/// Parse a MouseButton payload.
///
/// Payload layout:
/// [0] = button (0=Left 1=Right 2=Middle)
/// [1] = state  (0=Released 1=Pressed, api::input::KeyState repr)
fn parse_mouse_button(payload: &[u8]) -> Vec<Event> {
    if payload.len() < 2 {
        return Vec::new();
    }
    let btn   = payload[0];
    let state = payload[1];

    let button = match btn {
        0 => MouseButton::Left,
        1 => MouseButton::Right,
        2 => MouseButton::Middle,
        _ => return Vec::new(), // Back/Forward not in viui::MouseButton
    };

    // api::input::KeyState: Released=0, Pressed=1
    let pos = Point::new(0.0, 0.0); // position unknown at button level; caller must track
    let event = match state {
        STATE_RELEASED => Event::MouseRelease { pos, button },
        STATE_PRESSED  => Event::MousePress   { pos, button },
        _              => return Vec::new(),
    };
    alloc::vec![event]
}

/// Parse a MouseScroll payload.
///
/// Payload layout:
/// [0..4] = dx (i32 LE, ignored — only vertical scroll used in G1)
/// [4..8] = dy (i32 LE, positive = scroll down)
fn parse_mouse_scroll(payload: &[u8]) -> Vec<Event> {
    if payload.len() < 8 {
        return Vec::new();
    }
    let dy = i32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
    let pos = Point::new(0.0, 0.0); // position unknown at scroll level; caller must track
    alloc::vec![Event::Scroll { pos, delta_y: dy as f32 }]
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Decode the api::input::Modifiers bitmask into viui::event::Modifiers.
fn decode_modifiers(bits: u8) -> Modifiers {
    Modifiers {
        shift: (bits & MOD_SHIFT) != 0,
        ctrl:  (bits & MOD_CTRL)  != 0,
        alt:   (bits & MOD_ALT)   != 0,
    }
}

/// Map a KeySym discriminant value to a viui::KeyCode.
///
/// Returns `None` for keysyms that have no viui::KeyCode equivalent (e.g. Insert,
/// media keys, unknown values). The caller emits `Char` separately for printable keys.
fn keysym_to_keycode(keysym: u32) -> Option<KeyCode> {
    match keysym {
        KEYSYM_BACKSPACE => Some(KeyCode::Backspace),
        KEYSYM_DELETE    => Some(KeyCode::Delete),
        KEYSYM_RETURN    => Some(KeyCode::Enter),
        KEYSYM_TAB       => Some(KeyCode::Tab),
        KEYSYM_ESCAPE    => Some(KeyCode::Escape),
        KEYSYM_LEFT      => Some(KeyCode::Left),
        KEYSYM_RIGHT     => Some(KeyCode::Right),
        KEYSYM_UP        => Some(KeyCode::Up),
        KEYSYM_DOWN      => Some(KeyCode::Down),
        KEYSYM_HOME      => Some(KeyCode::Home),
        KEYSYM_END       => Some(KeyCode::End),
        KEYSYM_PAGE_UP   => Some(KeyCode::PageUp),
        KEYSYM_PAGE_DOWN => Some(KeyCode::PageDown),
        // F1..F12: keysym = 0x0100 + n (n = 1..12)
        k if k > KEYSYM_F_BASE && k <= KEYSYM_F_MAX => {
            Some(KeyCode::F((k - KEYSYM_F_BASE) as u8))
        }
        // Printable: emit no KeyCode here; Char is emitted by the caller
        KEYSYM_PRINTABLE => None,
        _ => None,
    }
}
