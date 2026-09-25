//! US QWERTY scancode → (KeySym, unshifted char, shifted char) mapping.
//!
//! Covers Linux evdev scancode set 1 (0x01..=0x7F).  Extended scancodes
//! (0x80..=0xFF, prefixed with 0xE0 on hardware) are handled separately via
//! `EXTENDED_TABLE`.
//!
//! The two lock keys that change what another key *means* are resolved here
//! rather than by the caller, because only the layout knows which keys they
//! reach: Caps Lock applies to the alphabetic keys alone
//! ([`LayoutEntry::is_alpha`]), and Num Lock chooses between a keypad key's
//! digit face and its navigation face ([`LayoutEntry::keypad`]).

use api::input::{KeyState, KeySym, Modifiers};

/// One entry in the layout table.
pub struct LayoutEntry {
    pub keysym: KeySym,
    pub unshifted: u32, // Unicode code point, 0 = non-printable
    pub shifted: u32,
    /// The `(keysym, character)` this key produces while Num Lock is off.
    ///
    /// `None` for every key Num Lock does not reach — the whole keyboard except
    /// the keypad. A keypad key keeps its digit face in `keysym`/`unshifted`
    /// (the Num Lock state) and its navigation face here.
    pub numlock_off: Option<(KeySym, u32)>,
}

impl LayoutEntry {
    const fn new(keysym: KeySym, u: u32, s: u32) -> Self {
        Self {
            keysym,
            unshifted: u,
            shifted: s,
            numlock_off: None,
        }
    }
    const fn ctrl(keysym: KeySym) -> Self {
        Self {
            keysym,
            unshifted: 0,
            shifted: 0,
            numlock_off: None,
        }
    }
    /// A keypad key: `keysym`/`u`/`s` with Num Lock on, `nav` with it off.
    const fn keypad(keysym: KeySym, u: u32, s: u32, nav: KeySym, nav_ch: u32) -> Self {
        Self {
            keysym,
            unshifted: u,
            shifted: s,
            numlock_off: Some((nav, nav_ch)),
        }
    }

    /// Whether Caps Lock changes this entry's case.
    ///
    /// Caps Lock is a *letter* lock: it shifts the alphabetic keys and leaves
    /// digits, punctuation and the keypad exactly as they are, which is what
    /// separates it from Shift. The layout already states which keys are
    /// letters — a lowercase unshifted character paired with its uppercase
    /// shifted one — so the answer is read off that pair instead of being kept
    /// in a second table that could drift away from it.
    fn is_alpha(&self) -> bool {
        let (Some(lower), Some(upper)) =
            (char::from_u32(self.unshifted), char::from_u32(self.shifted))
        else {
            return false;
        };
        lower.is_ascii_lowercase() && upper == lower.to_ascii_uppercase()
    }
}

/// Standard scancode table indexed by evdev code (0..=127).
/// Entry index = evdev EV_KEY code.
pub static LAYOUT: [LayoutEntry; 128] = [
    /* 0x00 */ LayoutEntry::ctrl(KeySym::Unknown),
    /* 0x01 */ LayoutEntry::ctrl(KeySym::Escape),
    /* 0x02 */ LayoutEntry::new(KeySym::Printable, b'1' as u32, b'!' as u32),
    /* 0x03 */ LayoutEntry::new(KeySym::Printable, b'2' as u32, b'@' as u32),
    /* 0x04 */ LayoutEntry::new(KeySym::Printable, b'3' as u32, b'#' as u32),
    /* 0x05 */ LayoutEntry::new(KeySym::Printable, b'4' as u32, b'$' as u32),
    /* 0x06 */ LayoutEntry::new(KeySym::Printable, b'5' as u32, b'%' as u32),
    /* 0x07 */ LayoutEntry::new(KeySym::Printable, b'6' as u32, b'^' as u32),
    /* 0x08 */ LayoutEntry::new(KeySym::Printable, b'7' as u32, b'&' as u32),
    /* 0x09 */ LayoutEntry::new(KeySym::Printable, b'8' as u32, b'*' as u32),
    /* 0x0A */ LayoutEntry::new(KeySym::Printable, b'9' as u32, b'(' as u32),
    /* 0x0B */ LayoutEntry::new(KeySym::Printable, b'0' as u32, b')' as u32),
    /* 0x0C */ LayoutEntry::new(KeySym::Printable, b'-' as u32, b'_' as u32),
    /* 0x0D */ LayoutEntry::new(KeySym::Printable, b'=' as u32, b'+' as u32),
    /* 0x0E */ LayoutEntry::ctrl(KeySym::Backspace),
    /* 0x0F */ LayoutEntry::ctrl(KeySym::Tab),
    /* 0x10 */ LayoutEntry::new(KeySym::Printable, b'q' as u32, b'Q' as u32),
    /* 0x11 */ LayoutEntry::new(KeySym::Printable, b'w' as u32, b'W' as u32),
    /* 0x12 */ LayoutEntry::new(KeySym::Printable, b'e' as u32, b'E' as u32),
    /* 0x13 */ LayoutEntry::new(KeySym::Printable, b'r' as u32, b'R' as u32),
    /* 0x14 */ LayoutEntry::new(KeySym::Printable, b't' as u32, b'T' as u32),
    /* 0x15 */ LayoutEntry::new(KeySym::Printable, b'y' as u32, b'Y' as u32),
    /* 0x16 */ LayoutEntry::new(KeySym::Printable, b'u' as u32, b'U' as u32),
    /* 0x17 */ LayoutEntry::new(KeySym::Printable, b'i' as u32, b'I' as u32),
    /* 0x18 */ LayoutEntry::new(KeySym::Printable, b'o' as u32, b'O' as u32),
    /* 0x19 */ LayoutEntry::new(KeySym::Printable, b'p' as u32, b'P' as u32),
    /* 0x1A */ LayoutEntry::new(KeySym::Printable, b'[' as u32, b'{' as u32),
    /* 0x1B */ LayoutEntry::new(KeySym::Printable, b']' as u32, b'}' as u32),
    /* 0x1C */ LayoutEntry::ctrl(KeySym::Return),
    /* 0x1D */ LayoutEntry::ctrl(KeySym::Unknown), // Left Ctrl
    /* 0x1E */ LayoutEntry::new(KeySym::Printable, b'a' as u32, b'A' as u32),
    /* 0x1F */ LayoutEntry::new(KeySym::Printable, b's' as u32, b'S' as u32),
    /* 0x20 */ LayoutEntry::new(KeySym::Printable, b'd' as u32, b'D' as u32),
    /* 0x21 */ LayoutEntry::new(KeySym::Printable, b'f' as u32, b'F' as u32),
    /* 0x22 */ LayoutEntry::new(KeySym::Printable, b'g' as u32, b'G' as u32),
    /* 0x23 */ LayoutEntry::new(KeySym::Printable, b'h' as u32, b'H' as u32),
    /* 0x24 */ LayoutEntry::new(KeySym::Printable, b'j' as u32, b'J' as u32),
    /* 0x25 */ LayoutEntry::new(KeySym::Printable, b'k' as u32, b'K' as u32),
    /* 0x26 */ LayoutEntry::new(KeySym::Printable, b'l' as u32, b'L' as u32),
    /* 0x27 */ LayoutEntry::new(KeySym::Printable, b';' as u32, b':' as u32),
    /* 0x28 */ LayoutEntry::new(KeySym::Printable, b'\'' as u32, b'"' as u32),
    /* 0x29 */ LayoutEntry::new(KeySym::Printable, b'`' as u32, b'~' as u32),
    /* 0x2A */ LayoutEntry::ctrl(KeySym::Unknown), // Left Shift
    /* 0x2B */ LayoutEntry::new(KeySym::Printable, b'\\' as u32, b'|' as u32),
    /* 0x2C */ LayoutEntry::new(KeySym::Printable, b'z' as u32, b'Z' as u32),
    /* 0x2D */ LayoutEntry::new(KeySym::Printable, b'x' as u32, b'X' as u32),
    /* 0x2E */ LayoutEntry::new(KeySym::Printable, b'c' as u32, b'C' as u32),
    /* 0x2F */ LayoutEntry::new(KeySym::Printable, b'v' as u32, b'V' as u32),
    /* 0x30 */ LayoutEntry::new(KeySym::Printable, b'b' as u32, b'B' as u32),
    /* 0x31 */ LayoutEntry::new(KeySym::Printable, b'n' as u32, b'N' as u32),
    /* 0x32 */ LayoutEntry::new(KeySym::Printable, b'm' as u32, b'M' as u32),
    /* 0x33 */ LayoutEntry::new(KeySym::Printable, b',' as u32, b'<' as u32),
    /* 0x34 */ LayoutEntry::new(KeySym::Printable, b'.' as u32, b'>' as u32),
    /* 0x35 */ LayoutEntry::new(KeySym::Printable, b'/' as u32, b'?' as u32),
    /* 0x36 */ LayoutEntry::ctrl(KeySym::Unknown), // Right Shift
    /* 0x37 */
    LayoutEntry::new(KeySym::Printable, b'*' as u32, b'*' as u32), // Keypad *
    /* 0x38 */ LayoutEntry::ctrl(KeySym::Unknown), // Left Alt
    /* 0x39 */ LayoutEntry::new(KeySym::Printable, b' ' as u32, b' ' as u32),
    /* 0x3A */ LayoutEntry::ctrl(KeySym::Unknown), // Caps Lock
    /* 0x3B */ LayoutEntry::ctrl(KeySym::F1),
    /* 0x3C */ LayoutEntry::ctrl(KeySym::F2),
    /* 0x3D */ LayoutEntry::ctrl(KeySym::F3),
    /* 0x3E */ LayoutEntry::ctrl(KeySym::F4),
    /* 0x3F */ LayoutEntry::ctrl(KeySym::F5),
    /* 0x40 */ LayoutEntry::ctrl(KeySym::F6),
    /* 0x41 */ LayoutEntry::ctrl(KeySym::F7),
    /* 0x42 */ LayoutEntry::ctrl(KeySym::F8),
    /* 0x43 */ LayoutEntry::ctrl(KeySym::F9),
    /* 0x44 */ LayoutEntry::ctrl(KeySym::F10),
    /* 0x45 */ LayoutEntry::ctrl(KeySym::Unknown), // Num Lock
    /* 0x46 */ LayoutEntry::ctrl(KeySym::Unknown), // Scroll Lock
    // 0x47..=0x53: the keypad. Each key is two keys in one — a digit while Num
    // Lock is on and a navigation key while it is off — so both faces are
    // carried here and `translate` picks. The operators (`-`, `+`, `*`, `/`)
    // have no navigation face and are the same key either way, which is why
    // they use the plain constructor.
    /* 0x47 */
    LayoutEntry::keypad(KeySym::Printable, b'7' as u32, b'7' as u32, KeySym::Home, 0),
    /* 0x48 */
    LayoutEntry::keypad(KeySym::Printable, b'8' as u32, b'8' as u32, KeySym::Up, 0),
    /* 0x49 */
    LayoutEntry::keypad(
        KeySym::Printable,
        b'9' as u32,
        b'9' as u32,
        KeySym::PageUp,
        0,
    ),
    /* 0x4A */ LayoutEntry::new(KeySym::Printable, b'-' as u32, b'-' as u32),
    /* 0x4B */
    LayoutEntry::keypad(KeySym::Printable, b'4' as u32, b'4' as u32, KeySym::Left, 0),
    // Num Lock off leaves keypad 5 with no navigation key of its own (X11 calls
    // it `KP_Begin`); `KeySym` has no such variant, so it is inert rather than
    // mapped onto a neighbour that would move a cursor.
    /* 0x4C */
    LayoutEntry::keypad(
        KeySym::Printable,
        b'5' as u32,
        b'5' as u32,
        KeySym::Unknown,
        0,
    ),
    /* 0x4D */
    LayoutEntry::keypad(
        KeySym::Printable,
        b'6' as u32,
        b'6' as u32,
        KeySym::Right,
        0,
    ),
    /* 0x4E */ LayoutEntry::new(KeySym::Printable, b'+' as u32, b'+' as u32),
    /* 0x4F */
    LayoutEntry::keypad(KeySym::Printable, b'1' as u32, b'1' as u32, KeySym::End, 0),
    /* 0x50 */
    LayoutEntry::keypad(KeySym::Printable, b'2' as u32, b'2' as u32, KeySym::Down, 0),
    /* 0x51 */
    LayoutEntry::keypad(
        KeySym::Printable,
        b'3' as u32,
        b'3' as u32,
        KeySym::PageDown,
        0,
    ),
    /* 0x52 */
    LayoutEntry::keypad(
        KeySym::Printable,
        b'0' as u32,
        b'0' as u32,
        KeySym::Insert,
        0,
    ),
    /* 0x53 */
    LayoutEntry::keypad(
        KeySym::Printable,
        b'.' as u32,
        b'.' as u32,
        KeySym::Delete,
        0,
    ),
    // 0x54..=0x56 (rare keys)
    /* 0x54 */ LayoutEntry::ctrl(KeySym::Unknown),
    /* 0x55 */ LayoutEntry::ctrl(KeySym::Unknown),
    /* 0x56 */ LayoutEntry::ctrl(KeySym::Unknown),
    /* 0x57 */ LayoutEntry::ctrl(KeySym::F11),
    /* 0x58 */ LayoutEntry::ctrl(KeySym::F12),
    // Remainder: unknown
    /* 0x59 */ LayoutEntry::ctrl(KeySym::Unknown),
    /* 0x5A */ LayoutEntry::ctrl(KeySym::Unknown),
    /* 0x5B */ LayoutEntry::ctrl(KeySym::Unknown),
    /* 0x5C */ LayoutEntry::ctrl(KeySym::Unknown),
    /* 0x5D */ LayoutEntry::ctrl(KeySym::Unknown),
    /* 0x5E */ LayoutEntry::ctrl(KeySym::Unknown),
    /* 0x5F */ LayoutEntry::ctrl(KeySym::Unknown),
    /* 0x60 */ LayoutEntry::ctrl(KeySym::Return), // evdev KEY_KPENTER (96)
    /* 0x61 */ LayoutEntry::ctrl(KeySym::Unknown),
    /* 0x62 */
    LayoutEntry::new(KeySym::Printable, b'/' as u32, b'/' as u32), // evdev KEY_KPSLASH (98)
    /* 0x63 */ LayoutEntry::ctrl(KeySym::Unknown),
    /* 0x64 */ LayoutEntry::ctrl(KeySym::Unknown),
    /* 0x65 */ LayoutEntry::ctrl(KeySym::Unknown),
    /* 0x66 */ LayoutEntry::ctrl(KeySym::Home), // evdev KEY_HOME     (102)
    /* 0x67 */ LayoutEntry::ctrl(KeySym::Up), // evdev KEY_UP       (103)
    /* 0x68 */ LayoutEntry::ctrl(KeySym::PageUp), // evdev KEY_PAGEUP   (104)
    /* 0x69 */ LayoutEntry::ctrl(KeySym::Left), // evdev KEY_LEFT     (105)
    /* 0x6A */ LayoutEntry::ctrl(KeySym::Right), // evdev KEY_RIGHT    (106)
    /* 0x6B */ LayoutEntry::ctrl(KeySym::End), // evdev KEY_END      (107)
    /* 0x6C */ LayoutEntry::ctrl(KeySym::Down), // evdev KEY_DOWN     (108)
    /* 0x6D */ LayoutEntry::ctrl(KeySym::PageDown), // evdev KEY_PAGEDOWN (109)
    /* 0x6E */ LayoutEntry::ctrl(KeySym::Unknown),
    /* 0x6F */ LayoutEntry::ctrl(KeySym::Unknown),
    /* 0x70 */ LayoutEntry::ctrl(KeySym::Unknown),
    /* 0x71 */ LayoutEntry::ctrl(KeySym::Unknown),
    /* 0x72 */ LayoutEntry::ctrl(KeySym::Unknown),
    /* 0x73 */ LayoutEntry::ctrl(KeySym::Unknown),
    /* 0x74 */ LayoutEntry::ctrl(KeySym::Unknown),
    /* 0x75 */
    LayoutEntry::new(KeySym::Printable, b'=' as u32, b'=' as u32), // evdev KEY_KPEQUAL (117)
    /* 0x76 */ LayoutEntry::ctrl(KeySym::Unknown),
    /* 0x77 */ LayoutEntry::ctrl(KeySym::Unknown),
    /* 0x78 */ LayoutEntry::ctrl(KeySym::Unknown),
    /* 0x79 */ LayoutEntry::ctrl(KeySym::Unknown),
    /* 0x7A */ LayoutEntry::ctrl(KeySym::Unknown),
    /* 0x7B */ LayoutEntry::ctrl(KeySym::Unknown),
    /* 0x7C */ LayoutEntry::ctrl(KeySym::Unknown),
    /* 0x7D */ LayoutEntry::ctrl(KeySym::Unknown),
    /* 0x7E */ LayoutEntry::ctrl(KeySym::Unknown),
    /* 0x7F */ LayoutEntry::ctrl(KeySym::Unknown),
];

/// Scancodes that update modifier state rather than producing a character.
/// Returns `Some(Modifiers)` for the modifier this scancode controls.
pub fn modifier_for_scancode(code: u32) -> Option<Modifiers> {
    match code {
        0x2A | 0x36 => Some(Modifiers::SHIFT), // Left/Right Shift
        0x1D | 0x61 => Some(Modifiers::CTRL),  // Left/Right Ctrl
        0x38 | 0x64 => Some(Modifiers::ALT),   // Left/Right Alt
        0x7D | 0x7E => Some(Modifiers::META),  // Left/Right Meta
        _ => None,
    }
}

/// Returns true if this scancode is a sticky-toggle modifier (Caps/Num/Scroll Lock).
pub fn toggle_modifier_for_scancode(code: u32) -> Option<Modifiers> {
    match code {
        0x3A => Some(Modifiers::CAPS_LOCK),
        0x45 => Some(Modifiers::NUM_LOCK),
        0x46 => Some(Modifiers::SCROLL_LOCK),
        _ => None,
    }
}

/// Translate a raw scancode + modifier state → (KeySym, Unicode char).
///
/// Returns `(KeySym::Unknown, 0)` for unrecognised scancodes.
pub fn translate(code: u32, modifiers: Modifiers) -> (KeySym, u32) {
    let idx = code as usize;
    if idx >= LAYOUT.len() {
        return (KeySym::Unknown, 0);
    }
    let entry = &LAYOUT[idx];

    // Num Lock decides which key a keypad key is before case is considered: the
    // navigation face has no shifted form, and Caps Lock never reaches it.
    if let Some((keysym, ch)) = entry.numlock_off {
        if !modifiers.contains(Modifiers::NUM_LOCK) {
            return (keysym, ch);
        }
    }

    // Effective shift = Shift XOR Caps Lock, but Caps Lock only shifts letters:
    // a digit stays a digit with Caps Lock on, which is the difference between
    // it and Shift.
    let caps = modifiers.contains(Modifiers::CAPS_LOCK) && entry.is_alpha();
    let shift_active = modifiers.contains(Modifiers::SHIFT) ^ caps;
    let ch = if shift_active {
        entry.shifted
    } else {
        entry.unshifted
    };
    (entry.keysym, ch)
}

/// Convert a `KeyState` from raw evdev value (0=release, 1=press, 2=repeat).
pub fn key_state_from_evdev(value: u32) -> KeyState {
    match value {
        0 => KeyState::Released,
        1 => KeyState::Pressed,
        _ => KeyState::Repeated,
    }
}

#[cfg(test)]
mod tests {
    use super::{toggle_modifier_for_scancode, translate};
    use api::input::{KeySym, Modifiers};

    /// The evdev codes the driver sends for the keys these tests press.
    const A: u32 = 0x1E;
    const DIGIT_1: u32 = 0x02;
    const MINUS: u32 = 0x0C;
    const SPACE: u32 = 0x39;
    const KP_0: u32 = 0x52;
    const KP_1: u32 = 0x4F;
    const KP_5: u32 = 0x4C;
    const KP_7: u32 = 0x47;
    const KP_DOT: u32 = 0x53;
    const KP_MINUS: u32 = 0x4A;
    const KP_SLASH: u32 = 0x62;
    const KP_ENTER: u32 = 0x60;

    fn mods(bits: &[Modifiers]) -> Modifiers {
        let mut m = Modifiers::default();
        for bit in bits {
            m.set(*bit);
        }
        m
    }

    fn ch(c: u8) -> u32 {
        c as u32
    }

    /// Caps Lock is a letter lock: `a` becomes `A`, and nothing else moves.
    #[test]
    fn caps_lock_shifts_letters_only() {
        let caps = mods(&[Modifiers::CAPS_LOCK]);
        assert_eq!(translate(A, caps), (KeySym::Printable, ch(b'A')));
        assert_eq!(translate(DIGIT_1, caps), (KeySym::Printable, ch(b'1')));
        assert_eq!(translate(MINUS, caps), (KeySym::Printable, ch(b'-')));
        assert_eq!(translate(SPACE, caps), (KeySym::Printable, ch(b' ')));
    }

    /// Shift still does what Caps Lock does not, and the two cancel on a letter.
    #[test]
    fn shift_and_caps_lock_cancel_on_a_letter() {
        assert_eq!(
            translate(A, mods(&[Modifiers::SHIFT])),
            (KeySym::Printable, ch(b'A'))
        );
        assert_eq!(
            translate(A, mods(&[Modifiers::SHIFT, Modifiers::CAPS_LOCK])),
            (KeySym::Printable, ch(b'a'))
        );
        // A digit's shifted character is Shift's alone.
        assert_eq!(
            translate(DIGIT_1, mods(&[Modifiers::SHIFT])),
            (KeySym::Printable, ch(b'!'))
        );
        assert_eq!(
            translate(DIGIT_1, mods(&[Modifiers::SHIFT, Modifiers::CAPS_LOCK])),
            (KeySym::Printable, ch(b'!'))
        );
    }

    /// Num Lock picks which key a keypad key is; the operators have only one.
    #[test]
    fn num_lock_picks_the_keypad_face() {
        let num = mods(&[Modifiers::NUM_LOCK]);
        let plain = Modifiers::default();

        assert_eq!(translate(KP_7, num), (KeySym::Printable, ch(b'7')));
        assert_eq!(translate(KP_7, plain), (KeySym::Home, 0));
        assert_eq!(translate(KP_1, num), (KeySym::Printable, ch(b'1')));
        assert_eq!(translate(KP_1, plain), (KeySym::End, 0));
        assert_eq!(translate(KP_0, plain), (KeySym::Insert, 0));
        assert_eq!(translate(KP_DOT, num), (KeySym::Printable, ch(b'.')));
        assert_eq!(translate(KP_DOT, plain), (KeySym::Delete, 0));

        // Keypad 5 has no navigation key of its own to fall back to.
        assert_eq!(translate(KP_5, num), (KeySym::Printable, ch(b'5')));
        assert_eq!(translate(KP_5, plain), (KeySym::Unknown, 0));

        // Operators are the same key with Num Lock either way.
        assert_eq!(translate(KP_MINUS, num), (KeySym::Printable, ch(b'-')));
        assert_eq!(translate(KP_MINUS, plain), (KeySym::Printable, ch(b'-')));
        assert_eq!(translate(KP_SLASH, plain), (KeySym::Printable, ch(b'/')));
        assert_eq!(translate(KP_ENTER, plain), (KeySym::Return, 0));
    }

    /// Caps Lock does not reach the keypad: a locked keypad still types digits.
    #[test]
    fn caps_lock_does_not_shift_the_keypad() {
        let both = mods(&[Modifiers::CAPS_LOCK, Modifiers::NUM_LOCK]);
        assert_eq!(translate(KP_1, both), (KeySym::Printable, ch(b'1')));
        assert_eq!(translate(KP_DOT, both), (KeySym::Printable, ch(b'.')));
    }

    /// The lock keys are toggles for the service, never characters.
    #[test]
    fn lock_keys_are_toggles_not_characters() {
        assert_eq!(
            toggle_modifier_for_scancode(0x3A),
            Some(Modifiers::CAPS_LOCK)
        );
        assert_eq!(
            toggle_modifier_for_scancode(0x45),
            Some(Modifiers::NUM_LOCK)
        );
        assert_eq!(
            toggle_modifier_for_scancode(0x46),
            Some(Modifiers::SCROLL_LOCK)
        );
        assert_eq!(toggle_modifier_for_scancode(A), None);
    }
}
