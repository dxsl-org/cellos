// SPDX-License-Identifier: MIT
//! Compositor/focus key events -> normalized terminal session input.
//!
//! Key codes and modifier bits follow the frozen `cellos-vt1` namespace in
//! `api::services::session` (named keys at `0x110000+`, characters as their own
//! code point). The input service turns them into bytes; this module never
//! produces escape sequences itself.

use api::services::session::{encode_key, key_code, modifiers};
use ostd::input::{KeyEvent, KeyState, KeySym, Modifiers};

/// One normalized input to submit to the session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostInput {
    /// Printable committed text (one character; never a control character).
    Text(char),
    /// Logical key: `pressed` is 1 for press, 2 for auto-repeat (0 emits no bytes).
    Key { code: u32, modifiers: u32, pressed: u8 },
}

/// What a key means while no shell child is running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdleAction {
    Restart,
    Close,
    None,
}

fn named_code(sym: KeySym) -> Option<u32> {
    Some(match sym {
        KeySym::Return => key_code::ENTER,
        KeySym::Escape => key_code::ESCAPE,
        KeySym::Backspace => key_code::BACKSPACE,
        KeySym::Tab => key_code::TAB,
        KeySym::Delete => key_code::DELETE,
        KeySym::Insert => key_code::INSERT,
        KeySym::Up => key_code::UP,
        KeySym::Down => key_code::DOWN,
        KeySym::Left => key_code::LEFT,
        KeySym::Right => key_code::RIGHT,
        KeySym::Home => key_code::HOME,
        KeySym::End => key_code::END,
        KeySym::PageUp => key_code::PAGE_UP,
        KeySym::PageDown => key_code::PAGE_DOWN,
        KeySym::F1 => key_code::F1,
        KeySym::F2 => key_code::F1 + 1,
        KeySym::F3 => key_code::F1 + 2,
        KeySym::F4 => key_code::F1 + 3,
        KeySym::F5 => key_code::F1 + 4,
        KeySym::F6 => key_code::F1 + 5,
        KeySym::F7 => key_code::F1 + 6,
        KeySym::F8 => key_code::F1 + 7,
        KeySym::F9 => key_code::F1 + 8,
        KeySym::F10 => key_code::F1 + 9,
        KeySym::F11 => key_code::F1 + 10,
        KeySym::F12 => key_code::F12,
        KeySym::Unknown | KeySym::Printable => return None,
    })
}

/// Map one compositor key event. `None` = nothing to submit (release, Meta chords,
/// control characters, and combinations the session cannot encode).
///
/// Return and Ctrl/Alt chords are one-shot: their auto-repeat is dropped so a held
/// Ctrl-C cannot keep cancelling and a held Enter cannot flood commands. Text,
/// Backspace and navigation keys repeat.
pub fn map_key(ke: &KeyEvent) -> Option<HostInput> {
    let pressed = match ke.state {
        KeyState::Released => return None,
        KeyState::Pressed => 1,
        KeyState::Repeated => 2,
    };
    let m = ke.modifiers;
    if m.contains(Modifiers::META) {
        return None;
    }
    let (ctrl, alt, shift) = (m.contains(Modifiers::CTRL), m.contains(Modifiers::ALT), m.contains(Modifiers::SHIFT));
    let chord = ctrl || alt;
    let mods = u32::from(shift) * modifiers::SHIFT
        | u32::from(ctrl) * modifiers::CONTROL
        | u32::from(alt) * modifiers::ALT;
    let repeated = pressed == 2;

    let input = if let Some(code) = named_code(ke.keysym) {
        if repeated && (chord || ke.keysym == KeySym::Return) {
            return None;
        }
        HostInput::Key { code, modifiers: mods, pressed }
    } else {
        let ch = ke.char()?;
        if ch.is_control() {
            return None;
        }
        if chord {
            if repeated {
                return None;
            }
            HostInput::Key { code: ch as u32, modifiers: mods, pressed }
        } else {
            HostInput::Text(ch)
        }
    };
    match input {
        HostInput::Key { code, modifiers, pressed } if encode_key(code, modifiers, pressed, &mut [0; 16]).is_err() => None,
        other => Some(other),
    }
}

/// Key handling while the child is gone: Enter restarts, Escape closes the app.
pub fn idle_action(ke: &KeyEvent) -> IdleAction {
    if ke.state != KeyState::Pressed {
        return IdleAction::None;
    }
    match ke.keysym {
        KeySym::Return => IdleAction::Restart,
        KeySym::Escape => IdleAction::Close,
        _ => IdleAction::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(keysym: KeySym, character: char, mods: u8, state: KeyState) -> KeyEvent {
        KeyEvent {
            timestamp_ticks: 0,
            scancode: 0,
            keysym,
            character: if character == '\0' { 0 } else { character as u32 },
            modifiers: Modifiers(mods),
            state,
            _pad: [0; 2],
        }
    }
    const CTRL: u8 = Modifiers::CTRL.0;
    const ALT: u8 = Modifiers::ALT.0;
    const SHIFT: u8 = Modifiers::SHIFT.0;
    const META: u8 = Modifiers::META.0;

    #[test]
    fn printable_text_is_submitted_as_text_including_repeat() {
        let a = ev(KeySym::Printable, 'a', 0, KeyState::Pressed);
        assert_eq!(map_key(&a), Some(HostInput::Text('a')));
        let shifted = ev(KeySym::Printable, 'A', SHIFT, KeyState::Repeated);
        assert_eq!(map_key(&shifted), Some(HostInput::Text('A')));
        let unicode = ev(KeySym::Printable, 'é', 0, KeyState::Pressed);
        assert_eq!(map_key(&unicode), Some(HostInput::Text('é')));
        // Lock modifiers do not turn text into a chord.
        let caps = ev(KeySym::Printable, 'A', Modifiers::CAPS_LOCK.0 | Modifiers::NUM_LOCK.0, KeyState::Pressed);
        assert_eq!(map_key(&caps), Some(HostInput::Text('A')));
    }

    #[test]
    fn release_meta_and_control_characters_submit_nothing() {
        assert_eq!(map_key(&ev(KeySym::Printable, 'a', 0, KeyState::Released)), None);
        assert_eq!(map_key(&ev(KeySym::Return, '\0', 0, KeyState::Released)), None);
        assert_eq!(map_key(&ev(KeySym::Printable, 'a', META, KeyState::Pressed)), None);
        assert_eq!(map_key(&ev(KeySym::Printable, '\x03', 0, KeyState::Pressed)), None);
        assert_eq!(map_key(&ev(KeySym::Unknown, '\0', 0, KeyState::Pressed)), None);
    }

    #[test]
    fn ctrl_c_is_the_exact_logical_key_the_input_service_treats_as_interrupt() {
        let c = ev(KeySym::Printable, 'c', CTRL, KeyState::Pressed);
        assert_eq!(map_key(&c), Some(HostInput::Key { code: 0x63, modifiers: 2, pressed: 1 }));
        // A held Ctrl-C is one-shot.
        assert_eq!(map_key(&ev(KeySym::Printable, 'c', CTRL, KeyState::Repeated)), None);
        let d = ev(KeySym::Printable, 'd', CTRL, KeyState::Pressed);
        assert_eq!(map_key(&d), Some(HostInput::Key { code: 0x64, modifiers: 2, pressed: 1 }));
    }

    #[test]
    fn alt_chords_and_unencodable_ctrl_combinations() {
        let alt_b = ev(KeySym::Printable, 'b', ALT, KeyState::Pressed);
        assert_eq!(map_key(&alt_b), Some(HostInput::Key { code: 0x62, modifiers: 4, pressed: 1 }));
        // Ctrl+1 has no control byte: refused here rather than at the owner.
        assert_eq!(map_key(&ev(KeySym::Printable, '1', CTRL, KeyState::Pressed)), None);
        // Ctrl with Enter/Backspace/Escape/Tab is not encodable.
        assert_eq!(map_key(&ev(KeySym::Return, '\0', CTRL, KeyState::Pressed)), None);
        assert_eq!(map_key(&ev(KeySym::Backspace, '\0', CTRL, KeyState::Pressed)), None);
    }

    #[test]
    fn named_keys_use_the_frozen_codes() {
        let k = |sym| map_key(&ev(sym, '\0', 0, KeyState::Pressed));
        assert_eq!(k(KeySym::Return), Some(HostInput::Key { code: 0x110001, modifiers: 0, pressed: 1 }));
        assert_eq!(k(KeySym::Escape), Some(HostInput::Key { code: 0x110002, modifiers: 0, pressed: 1 }));
        assert_eq!(k(KeySym::Backspace), Some(HostInput::Key { code: 0x110003, modifiers: 0, pressed: 1 }));
        assert_eq!(k(KeySym::Tab), Some(HostInput::Key { code: 0x110004, modifiers: 0, pressed: 1 }));
        assert_eq!(k(KeySym::Delete), Some(HostInput::Key { code: 0x110005, modifiers: 0, pressed: 1 }));
        assert_eq!(k(KeySym::Up), Some(HostInput::Key { code: 0x110010, modifiers: 0, pressed: 1 }));
        assert_eq!(k(KeySym::Down), Some(HostInput::Key { code: 0x110011, modifiers: 0, pressed: 1 }));
        assert_eq!(k(KeySym::Left), Some(HostInput::Key { code: 0x110012, modifiers: 0, pressed: 1 }));
        assert_eq!(k(KeySym::Right), Some(HostInput::Key { code: 0x110013, modifiers: 0, pressed: 1 }));
        assert_eq!(k(KeySym::Home), Some(HostInput::Key { code: 0x110014, modifiers: 0, pressed: 1 }));
        assert_eq!(k(KeySym::End), Some(HostInput::Key { code: 0x110015, modifiers: 0, pressed: 1 }));
        assert_eq!(k(KeySym::PageUp), Some(HostInput::Key { code: 0x110016, modifiers: 0, pressed: 1 }));
        assert_eq!(k(KeySym::PageDown), Some(HostInput::Key { code: 0x110017, modifiers: 0, pressed: 1 }));
        assert_eq!(k(KeySym::F1), Some(HostInput::Key { code: 0x110020, modifiers: 0, pressed: 1 }));
        assert_eq!(k(KeySym::F5), Some(HostInput::Key { code: 0x110024, modifiers: 0, pressed: 1 }));
        assert_eq!(k(KeySym::F12), Some(HostInput::Key { code: 0x11002b, modifiers: 0, pressed: 1 }));
    }

    #[test]
    fn modifiers_map_to_session_bits_and_named_repeat_policy() {
        let up = ev(KeySym::Up, '\0', SHIFT | CTRL | ALT, KeyState::Pressed);
        assert_eq!(map_key(&up), Some(HostInput::Key { code: 0x110010, modifiers: 7, pressed: 1 }));
        // Navigation and Backspace repeat; Return and chorded keys do not.
        assert_eq!(map_key(&ev(KeySym::Backspace, '\0', 0, KeyState::Repeated)), Some(HostInput::Key { code: 0x110003, modifiers: 0, pressed: 2 }));
        assert_eq!(map_key(&ev(KeySym::Left, '\0', 0, KeyState::Repeated)), Some(HostInput::Key { code: 0x110012, modifiers: 0, pressed: 2 }));
        assert_eq!(map_key(&ev(KeySym::Return, '\0', 0, KeyState::Repeated)), None);
        assert_eq!(map_key(&ev(KeySym::Left, '\0', CTRL, KeyState::Repeated)), None);
    }

    #[test]
    fn every_mapped_key_is_accepted_by_the_session_validator() {
        let syms = [
            KeySym::Return, KeySym::Escape, KeySym::Backspace, KeySym::Tab, KeySym::Delete, KeySym::Insert,
            KeySym::Up, KeySym::Down, KeySym::Left, KeySym::Right, KeySym::Home, KeySym::End,
            KeySym::PageUp, KeySym::PageDown, KeySym::F1, KeySym::F2, KeySym::F3, KeySym::F4, KeySym::F5,
            KeySym::F6, KeySym::F7, KeySym::F8, KeySym::F9, KeySym::F10, KeySym::F11, KeySym::F12,
        ];
        for sym in syms {
            let Some(HostInput::Key { code, modifiers, pressed }) = map_key(&ev(sym, '\0', 0, KeyState::Pressed)) else {
                panic!("{sym:?} must map");
            };
            let mut payload = [0u8; 16];
            payload[..4].copy_from_slice(&code.to_le_bytes());
            payload[4..8].copy_from_slice(&modifiers.to_le_bytes());
            payload[8] = pressed;
            assert!(api::services::session::validate_event(1, &payload).is_ok(), "{sym:?}");
        }
    }

    #[test]
    fn idle_keys_restart_or_close_only_on_press() {
        assert_eq!(idle_action(&ev(KeySym::Return, '\0', 0, KeyState::Pressed)), IdleAction::Restart);
        assert_eq!(idle_action(&ev(KeySym::Return, '\0', 0, KeyState::Repeated)), IdleAction::None);
        assert_eq!(idle_action(&ev(KeySym::Return, '\0', 0, KeyState::Released)), IdleAction::None);
        assert_eq!(idle_action(&ev(KeySym::Escape, '\0', 0, KeyState::Pressed)), IdleAction::Close);
        assert_eq!(idle_action(&ev(KeySym::Printable, 'x', 0, KeyState::Pressed)), IdleAction::None);
    }
}
