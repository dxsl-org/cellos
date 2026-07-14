// SPDX-License-Identifier: MIT
//! Converts `ostd::input` events to `viui::Event`.
//!
//! `request_input_focus` and `collect_input_events` delegate to `ostd::input`
//! for service-discovery and IPC; this module handles only the
//! `api::input::InputEvent` → `viui::Event` mapping.
//!
//! A Pressed/Repeated key with a printable character emits BOTH `KeyPress` AND
//! `Char(c)` so text-edit widgets and shortcut handlers both fire. A Released
//! key emits `KeyRelease` so apps can detect key-up (e.g. held-key state).

extern crate alloc;
use alloc::vec::Vec;

use crate::event::{Event, KeyCode, Modifiers, MouseButton};
use crate::layout::Point;
use api::input::{InputEvent, KeyEvent, KeyState};

// ─── Modifier bit constants (api::input::Modifiers bitmask) ──────────────────

const MOD_SHIFT: u8 = 0b0000_0001;
const MOD_CTRL: u8 = 0b0000_0010;
const MOD_ALT: u8 = 0b0000_0100;

// ─── Public API ──────────────────────────────────────────────────────────────

/// Register this cell as the focused input receiver.
///
/// Delegates to `ostd::input::request_focus`. The input service uses the
/// kernel-verified sender TID, preventing TID impersonation.
/// Returns `true` when focus is granted; `false` on boot race or error.
pub fn request_input_focus() -> bool {
    ostd::input::request_focus()
}

/// Drain pending input events from the input service (non-blocking).
///
/// Calls `ostd::input::poll_events(max_events)` then maps each
/// `api::input::InputEvent` to the equivalent `viui::Event`(s).
/// Typically called once per frame before `ViApp::tick_with_dt`.
pub fn collect_input_events(max_events: usize) -> Vec<Event> {
    ostd::input::poll_events(max_events)
        .iter()
        .flat_map(input_event_to_viui)
        .collect()
}

// ─── api::input::InputEvent → viui::Event conversion ────────────────────────

fn input_event_to_viui(ev: &InputEvent) -> Vec<Event> {
    match ev {
        InputEvent::Key(k) => convert_key(k),
        InputEvent::MouseMove { x, y, .. } => convert_mouse_move(*x, *y),
        InputEvent::MouseButton { button, state } => convert_mouse_button(button, state),
        InputEvent::MouseScroll { dy, .. } => convert_mouse_scroll(*dy),
    }
}

/// Convert a `KeyEvent` into viui events.
///
/// - Released → `KeyRelease { key }`
/// - Pressed/Repeated → `KeyPress { key, modifiers }` + optional `Char(c)`
fn convert_key(k: &KeyEvent) -> Vec<Event> {
    let mods = decode_modifiers(k.modifiers.0);
    let keysym_u32 = k.keysym as u32;

    match k.state {
        KeyState::Released => {
            if let Some(key) = keysym_to_keycode(keysym_u32) {
                return alloc::vec![Event::KeyRelease { key }];
            }
            Vec::new()
        }
        KeyState::Pressed | KeyState::Repeated => {
            let mut events = Vec::with_capacity(2);
            if let Some(key) = keysym_to_keycode(keysym_u32) {
                events.push(Event::KeyPress {
                    key,
                    modifiers: mods,
                });
            }
            // Emit Char for printable characters; skip if Ctrl/Alt held (shortcuts).
            if k.character != 0 && !mods.ctrl && !mods.alt {
                if let Some(c) = char::from_u32(k.character) {
                    events.push(Event::Char(c));
                }
            }
            events
        }
    }
}

fn convert_mouse_move(x: i32, y: i32) -> Vec<Event> {
    let pos = Point::new(x.max(0) as f32, y.max(0) as f32);
    alloc::vec![Event::MouseMove { pos }]
}

fn convert_mouse_button(button: &api::input::MouseButton, state: &KeyState) -> Vec<Event> {
    let btn = match button {
        api::input::MouseButton::Left => MouseButton::Left,
        api::input::MouseButton::Right => MouseButton::Right,
        api::input::MouseButton::Middle => MouseButton::Middle,
        // Back/Forward have no viui::MouseButton equivalent — drop silently.
        _ => return Vec::new(),
    };
    let pos = Point::new(0.0, 0.0); // position unknown; callers must track separately
    let ev = match state {
        KeyState::Released => Event::MouseRelease { pos, button: btn },
        KeyState::Pressed => Event::MousePress { pos, button: btn },
        _ => return Vec::new(),
    };
    alloc::vec![ev]
}

fn convert_mouse_scroll(dy: i32) -> Vec<Event> {
    let pos = Point::new(0.0, 0.0);
    alloc::vec![Event::Scroll {
        pos,
        delta_y: dy as f32
    }]
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn decode_modifiers(bits: u8) -> Modifiers {
    Modifiers {
        shift: (bits & MOD_SHIFT) != 0,
        ctrl: (bits & MOD_CTRL) != 0,
        alt: (bits & MOD_ALT) != 0,
    }
}

fn keysym_to_keycode(keysym: u32) -> Option<KeyCode> {
    match keysym {
        0x0003 => Some(KeyCode::Backspace),
        0x0005 => Some(KeyCode::Delete),
        0x0002 => Some(KeyCode::Enter),
        0x0004 => Some(KeyCode::Tab),
        0x0001 => Some(KeyCode::Escape),
        0x0012 => Some(KeyCode::Left),
        0x0013 => Some(KeyCode::Right),
        0x0010 => Some(KeyCode::Up),
        0x0011 => Some(KeyCode::Down),
        0x0020 => Some(KeyCode::Home),
        0x0021 => Some(KeyCode::End),
        0x0022 => Some(KeyCode::PageUp),
        0x0023 => Some(KeyCode::PageDown),
        k if k > 0x0100 && k <= 0x010C => Some(KeyCode::F((k - 0x0100) as u8)),
        0x8000 => None, // Printable — Char emitted by the caller
        _ => None,
    }
}
