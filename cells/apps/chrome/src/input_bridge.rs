//! The window's input as Linux evdev events for the guest's virtio-input tablet.
//!
//! Key events carry the evdev code in `KeyEvent::scancode` (the host's own
//! virtio keyboard codes). Modifier keys are not delivered to apps as separate
//! events, only as `KeyEvent::modifiers`, so the guest's modifier keys are
//! pressed and released to match before each key.

use alloc::vec::Vec;

use api::services::vmm_session::{
    abs_position, ABS_X, ABS_Y, BTN_EXTRA, BTN_LEFT, BTN_MIDDLE, BTN_RIGHT, BTN_SIDE, EV_ABS,
    EV_KEY, EV_REL, REL_WHEEL,
};
use ostd::input::{InputEvent, KeyState, Modifiers, MouseButton};

/// One evdev event: (type, code, value).
pub type GuestEvent = (u16, u16, i32);

/// Linux keycodes of the modifiers kept in step with `KeyEvent::modifiers`.
const MODIFIER_KEYS: [(Modifiers, u16); 4] = [
    (Modifiers::SHIFT, 42), // KEY_LEFTSHIFT
    (Modifiers::CTRL, 29),  // KEY_LEFTCTRL
    (Modifiers::ALT, 56),   // KEY_LEFTALT
    (Modifiers::META, 125), // KEY_LEFTMETA
];
/// Highest evdev key code the guest tablet accepts (KEY_MAX).
const KEY_MAX: u32 = 0x2ff;

pub struct InputBridge {
    /// Modifiers currently held down in the guest.
    held: Modifiers,
}

impl InputBridge {
    pub const fn new() -> Self {
        Self {
            held: Modifiers(0),
        }
    }

    /// Guest events for one window event; `width`/`height` are the window's
    /// content size, which the guest display matches.
    pub fn translate(&mut self, event: &InputEvent, width: u32, height: u32) -> Vec<GuestEvent> {
        let mut out = Vec::new();
        match *event {
            InputEvent::MouseMove { x, y, .. } => {
                out.push((EV_ABS, ABS_X, abs_position(x, width)));
                out.push((EV_ABS, ABS_Y, abs_position(y, height)));
            }
            InputEvent::MouseButton { button, state } => {
                out.push((EV_KEY, button_code(button), i32::from(state != KeyState::Released)));
            }
            InputEvent::MouseScroll { dy, .. } if dy != 0 => {
                // Host scroll is positive downwards; evdev REL_WHEEL is positive upwards.
                out.push((EV_REL, REL_WHEEL, -dy));
            }
            InputEvent::MouseScroll { .. } => {}
            InputEvent::Key(key) => {
                if key.scancode == 0 || key.scancode > KEY_MAX {
                    return out;
                }
                self.sync_modifiers(key.modifiers, &mut out);
                let code = key.scancode as u16;
                match key.state {
                    KeyState::Released => out.push((EV_KEY, code, 0)),
                    KeyState::Pressed => out.push((EV_KEY, code, 1)),
                    // The host input service owns repeat and the guest's is off
                    // (scripts/chrome-guest/no-key-repeat.c). libinput drops
                    // evdev value 2, so a repeat is a fresh press.
                    KeyState::Repeated => out.extend([(EV_KEY, code, 0), (EV_KEY, code, 1)]),
                }
            }
        }
        out
    }

    fn sync_modifiers(&mut self, wanted: Modifiers, out: &mut Vec<GuestEvent>) {
        for (modifier, code) in MODIFIER_KEYS {
            let want = wanted.contains(modifier);
            if want != self.held.contains(modifier) {
                out.push((EV_KEY, code, i32::from(want)));
                self.held = Modifiers(self.held.0 ^ modifier.0);
            }
        }
    }
}

fn button_code(button: MouseButton) -> u16 {
    match button {
        MouseButton::Left => BTN_LEFT,
        MouseButton::Right => BTN_RIGHT,
        MouseButton::Middle => BTN_MIDDLE,
        MouseButton::Back => BTN_SIDE,
        MouseButton::Forward => BTN_EXTRA,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use api::input::KeyEvent;
    use api::services::vmm_session::ABS_MAX;

    fn key(scancode: u32, modifiers: Modifiers, state: KeyState) -> InputEvent {
        InputEvent::Key(KeyEvent {
            timestamp_ticks: 0,
            scancode,
            keysym: api::input::KeySym::Printable,
            character: 0,
            modifiers,
            state,
            _pad: [0; 2],
        })
    }

    #[test]
    fn shifted_key_presses_shift_first_and_releases_it_with_the_next_plain_key() {
        let mut bridge = InputBridge::new();
        let shifted = bridge.translate(&key(30, Modifiers::SHIFT, KeyState::Pressed), 800, 600);
        assert_eq!(shifted, [(EV_KEY, 42, 1), (EV_KEY, 30, 1)]);
        let repeat = bridge.translate(&key(30, Modifiers::SHIFT, KeyState::Repeated), 800, 600);
        assert_eq!(repeat, [(EV_KEY, 30, 0), (EV_KEY, 30, 1)]);
        let plain = bridge.translate(&key(48, Modifiers(0), KeyState::Pressed), 800, 600);
        assert_eq!(plain, [(EV_KEY, 42, 0), (EV_KEY, 48, 1)]);
    }

    #[test]
    fn pointer_maps_window_corners_to_the_axis_ends() {
        let mut bridge = InputBridge::new();
        let corner = InputEvent::MouseMove {
            x: 799,
            y: 0,
            dx: 0,
            dy: 0,
        };
        assert_eq!(
            bridge.translate(&corner, 800, 600),
            [(EV_ABS, ABS_X, ABS_MAX), (EV_ABS, ABS_Y, 0)]
        );
        let scroll = InputEvent::MouseScroll { dx: 0, dy: 3 };
        assert_eq!(bridge.translate(&scroll, 800, 600), [(EV_REL, REL_WHEEL, -3)]);
    }

    #[test]
    fn keys_without_an_evdev_code_are_not_forwarded() {
        let mut bridge = InputBridge::new();
        assert!(bridge
            .translate(&key(0, Modifiers::SHIFT, KeyState::Pressed), 800, 600)
            .is_empty());
    }
}
