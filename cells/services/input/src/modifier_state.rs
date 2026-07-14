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

    /// Reset all non-lock modifiers.  Called on focus change to prevent stuck keys.
    pub fn reset_transient(&mut self) {
        self.0.clear(Modifiers::SHIFT);
        self.0.clear(Modifiers::CTRL);
        self.0.clear(Modifiers::ALT);
        self.0.clear(Modifiers::META);
    }
}
