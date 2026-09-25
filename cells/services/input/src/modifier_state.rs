//! Modifier key state machine — tracks Shift, Ctrl, Alt, Meta, and lock keys.
//!
//! `ModifierState` is updated on every raw scancode event before translation.
//! Sticky keys (Caps/Num/Scroll Lock) toggle on press; shift/ctrl/alt/meta
//! track press/release symmetrically.

use crate::layout_us_qwerty::{modifier_for_scancode, toggle_modifier_for_scancode};
use api::input::{KeyState, Modifiers};

/// Tracks the current state of all modifier keys.
#[derive(Debug, Default, Clone, Copy)]
pub struct ModifierState(pub Modifiers);

impl ModifierState {
    pub fn new() -> Self {
        Self(Modifiers::default())
    }

    /// Update state based on a raw scancode event.
    ///
    /// Call this BEFORE translating the scancode to a `KeyEvent`.  Returns
    /// true if the scancode was consumed as a modifier (no `KeyEvent` should
    /// be emitted for pure modifier keys).
    pub fn update(&mut self, scancode: u32, state: KeyState) -> bool {
        // Sticky toggles (Caps/Num/Scroll Lock) — toggle on key press only.
        if let Some(m) = toggle_modifier_for_scancode(scancode) {
            if state == KeyState::Pressed {
                self.0.toggle(m);
            }
            return true;
        }

        // Regular modifiers: set on press, clear on release.
        if let Some(m) = modifier_for_scancode(scancode) {
            match state {
                KeyState::Pressed | KeyState::Repeated => self.0.set(m),
                KeyState::Released => self.0.clear(m),
            }
            return true;
        }

        false
    }

    /// Current modifier snapshot (copied into each `KeyEvent`).
    pub fn snapshot(&self) -> Modifiers {
        self.0
    }

    /// Lock state as the keyboard LED bitmap the wire carries.
    ///
    /// The lock keys are the one piece of keyboard state the host has to write
    /// back: a keyboard shows Caps/Num/Scroll Lock on its own LEDs and nothing
    /// else can light them. The bits are the ones [`api::ipc::OP_SET_LEDS`]
    /// defines, so this value goes out unchanged.
    pub fn led_bitmap(&self) -> u8 {
        let mut bits = 0u8;
        if self.0.contains(Modifiers::NUM_LOCK) {
            bits |= api::ipc::led_bits::NUM_LOCK;
        }
        if self.0.contains(Modifiers::CAPS_LOCK) {
            bits |= api::ipc::led_bits::CAPS_LOCK;
        }
        if self.0.contains(Modifiers::SCROLL_LOCK) {
            bits |= api::ipc::led_bits::SCROLL_LOCK;
        }
        bits
    }

    /// Reset all non-lock modifiers.  Called on focus change to prevent stuck keys.
    pub fn reset_transient(&mut self) {
        self.0.clear(Modifiers::SHIFT);
        self.0.clear(Modifiers::CTRL);
        self.0.clear(Modifiers::ALT);
        self.0.clear(Modifiers::META);
    }
}

#[cfg(test)]
mod tests {
    use super::ModifierState;
    use api::input::{KeyState, Modifiers};
    use api::ipc::led_bits;

    const CAPS_LOCK: u32 = 0x3A;
    const NUM_LOCK: u32 = 0x45;
    const SCROLL_LOCK: u32 = 0x46;
    const SHIFT: u32 = 0x2A;

    /// A lock key toggles on press, its release changes nothing, and the LED
    /// bitmap the producers receive follows it.
    #[test]
    fn lock_keys_toggle_and_report_the_led_bitmap() {
        let mut state = ModifierState::new();
        assert_eq!(state.led_bitmap(), 0);

        assert!(state.update(CAPS_LOCK, KeyState::Pressed));
        assert_eq!(state.led_bitmap(), led_bits::CAPS_LOCK);
        assert!(state.update(CAPS_LOCK, KeyState::Released));
        assert_eq!(state.led_bitmap(), led_bits::CAPS_LOCK);

        assert!(state.update(NUM_LOCK, KeyState::Pressed));
        assert_eq!(state.led_bitmap(), led_bits::CAPS_LOCK | led_bits::NUM_LOCK);
        assert!(state.update(NUM_LOCK, KeyState::Pressed));
        assert_eq!(state.led_bitmap(), led_bits::CAPS_LOCK);

        assert!(state.update(SCROLL_LOCK, KeyState::Pressed));
        assert_eq!(
            state.led_bitmap(),
            led_bits::CAPS_LOCK | led_bits::SCROLL_LOCK
        );
    }

    /// A focus change drops held modifiers but keeps the locks: the keyboard's
    /// LEDs must keep showing what the locks are set to.
    #[test]
    fn focus_change_keeps_the_locks() {
        let mut state = ModifierState::new();
        state.update(CAPS_LOCK, KeyState::Pressed);
        state.update(SHIFT, KeyState::Pressed);

        state.reset_transient();

        assert_eq!(state.snapshot(), Modifiers::CAPS_LOCK);
        assert_eq!(state.led_bitmap(), led_bits::CAPS_LOCK);
    }

    /// A key that is not a modifier is left for translation.
    #[test]
    fn ordinary_keys_are_not_consumed() {
        let mut state = ModifierState::new();
        assert!(!state.update(0x1E, KeyState::Pressed));
        assert_eq!(state.snapshot(), Modifiers::default());
    }
}
