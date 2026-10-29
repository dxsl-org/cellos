//! One repeat owner for all keyboard producers; no allocation or catch-up bursts.
use api::input::{KeyState, KeySym};

const DELAY_MS: u64 = 400;
const INTERVAL_MS: u64 = 33;

#[derive(Default)]
pub(crate) struct KeyRepeat {
    held: Option<(Option<u32>, u32)>,
    deadline_ms: u64,
}

impl KeyRepeat {
    pub(crate) fn clear(&mut self) {
        self.held = None;
    }

    pub(crate) fn remove_device(&mut self, device: u32) {
        if self.held.is_some_and(|(owner, _)| owner == Some(device)) {
            self.clear();
        }
    }

    /// Hardware repeat is suppressed: the service owns cadence on every backend.
    pub(crate) fn observe(
        &mut self,
        device: Option<u32>,
        code: u32,
        state: KeyState,
        keysym: KeySym,
        now: u64,
    ) -> bool {
        let key = (device, code);
        match state {
            KeyState::Released => {
                if self.held == Some(key) {
                    self.clear();
                }
            }
            KeyState::Repeated => return false,
            KeyState::Pressed => {
                if self.held == Some(key) {
                    return false;
                }
                self.clear();
                if matches!(
                    keysym,
                    KeySym::Printable
                        | KeySym::Backspace
                        | KeySym::Delete
                        | KeySym::Return
                        | KeySym::Tab
                        | KeySym::Up
                        | KeySym::Down
                        | KeySym::Left
                        | KeySym::Right
                        | KeySym::Home
                        | KeySym::End
                        | KeySym::PageUp
                        | KeySym::PageDown
                        | KeySym::F3
                ) {
                    self.held = Some(key);
                    self.deadline_ms = now.saturating_add(DELAY_MS);
                }
            }
        }
        true
    }

    pub(crate) fn poll(&mut self, now: u64) -> Option<u32> {
        let (_, code) = self.held?;
        if now < self.deadline_ms {
            return None;
        }
        self.deadline_ms = now.saturating_add(INTERVAL_MS);
        Some(code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backspace_repeats_until_release_without_catch_up_bursts() {
        let mut r = KeyRepeat::default();
        assert!(r.observe(None, 14, KeyState::Pressed, KeySym::Backspace, 10));
        assert_eq!(r.poll(409), None);
        assert_eq!(r.poll(410), Some(14));
        assert_eq!(r.poll(442), None);
        assert_eq!(r.poll(443), Some(14));
        assert_eq!(r.poll(900), Some(14));
        assert_eq!(r.poll(900), None);
        r.observe(None, 14, KeyState::Released, KeySym::Backspace, 901);
        assert_eq!(r.poll(2000), None);
    }

    #[test]
    fn last_key_wins_and_backend_repeat_does_not_double_or_delay_it() {
        let mut r = KeyRepeat::default();
        r.observe(None, 14, KeyState::Pressed, KeySym::Backspace, 0);
        assert!(!r.observe(None, 14, KeyState::Repeated, KeySym::Backspace, 100));
        assert!(!r.observe(None, 14, KeyState::Pressed, KeySym::Backspace, 200));
        assert_eq!(r.poll(400), Some(14));
        r.observe(None, 30, KeyState::Pressed, KeySym::Printable, 410);
        r.observe(None, 14, KeyState::Released, KeySym::Backspace, 420);
        assert_eq!(r.poll(810), Some(30));
        r.clear();
        assert_eq!(r.poll(2000), None);
    }

    #[test]
    fn disconnect_cancels_only_owner_and_toggle_keys_do_not_repeat() {
        let mut r = KeyRepeat::default();
        r.observe(Some(1), 14, KeyState::Pressed, KeySym::Backspace, 0);
        r.remove_device(2);
        assert_eq!(r.poll(400), Some(14));
        r.remove_device(1);
        assert_eq!(r.poll(1000), None);
        r.observe(None, 59, KeyState::Pressed, KeySym::F1, 1000);
        assert_eq!(r.poll(2000), None);
        r.observe(None, 14, KeyState::Pressed, KeySym::Backspace, 2100);
        r.observe(Some(1), 14, KeyState::Released, KeySym::Backspace, 2200);
        assert_eq!(r.poll(2500), Some(14));
        r.observe(None, 61, KeyState::Pressed, KeySym::F3, 2600);
        assert_eq!(r.poll(3000), Some(61));
    }
}
