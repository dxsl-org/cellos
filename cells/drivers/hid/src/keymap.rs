//! HID Usage ID → Linux evdev code mapping.
//!
//! The input service (`cells/services/input`) consumes Linux evdev `EV_KEY`
//! codes: its US-QWERTY layout table is indexed by evdev code and its
//! modifier/navigation entries match `input-event-codes.h` exactly. So a HID
//! device only needs the same translation Linux' `hid-input.c` performs —
//! Usage Page 0x07 (Keyboard/Keypad) usage → `KEY_*`, page 0x09 (Button)
//! usage → `BTN_*`, page 0x01 (Generic Desktop) X/Y/Wheel → `REL_*`.
//!
//! Codes here are the kernel-numbered evdev values, not scan codes.

// ─── Mouse buttons (linux/input-event-codes.h BTN_*) ─────────────────────────
pub const BTN_LEFT: u32 = 0x110;
pub const BTN_RIGHT: u32 = 0x111;
pub const BTN_MIDDLE: u32 = 0x112;
pub const BTN_SIDE: u32 = 0x113;
pub const BTN_EXTRA: u32 = 0x114;
pub const BTN_FORWARD: u32 = 0x115;
pub const BTN_BACK: u32 = 0x116;
pub const BTN_TASK: u32 = 0x117;

// ─── Relative axes (linux/input-event-codes.h REL_*) ─────────────────────────
pub const REL_X: u32 = 0x00;
pub const REL_Y: u32 = 0x01;
pub const REL_Z: u32 = 0x02;
pub const REL_HWHEEL: u32 = 0x06;
pub const REL_WHEEL: u32 = 0x08;

// ─── HID usage pages ─────────────────────────────────────────────────────────
pub const PAGE_GENERIC_DESKTOP: u32 = 0x01;
pub const PAGE_KEYBOARD: u32 = 0x07;
pub const PAGE_LED: u32 = 0x08;
pub const PAGE_BUTTON: u32 = 0x09;
pub const PAGE_CONSUMER: u32 = 0x0C;

// ─── LED-page usages (HID 1.11 §11.7, the host→device direction) ─────────────
/// Keyboard LED usages, in the order the boot-protocol LED byte reports them.
pub const LED_NUM_LOCK: u32 = 0x01;
pub const LED_CAPS_LOCK: u32 = 0x02;
pub const LED_SCROLL_LOCK: u32 = 0x03;

// ─── Generic Desktop usages ──────────────────────────────────────────────────
pub const GD_POINTER: u32 = 0x01;
pub const GD_MOUSE: u32 = 0x02;
pub const GD_KEYBOARD: u32 = 0x06;
pub const GD_X: u32 = 0x30;
pub const GD_Y: u32 = 0x31;
pub const GD_Z: u32 = 0x32;
pub const GD_WHEEL: u32 = 0x38;
pub const GD_HWHEEL: u32 = 0x3E;

/// Map a HID Generic Desktop usage to its evdev relative-axis code.
///
/// Returns `None` for a usage this driver does not forward (absolute axes and
/// vendor usages are dropped rather than guessed at).
pub fn generic_desktop_to_rel(usage: u32) -> Option<u32> {
    match usage {
        GD_X => Some(REL_X),
        GD_Y => Some(REL_Y),
        GD_Z => Some(REL_Z),
        GD_WHEEL => Some(REL_WHEEL),
        GD_HWHEEL => Some(REL_HWHEEL),
        _ => None,
    }
}

/// Map a HID Button-page usage (1-based) to its evdev `BTN_*` code.
pub fn button_to_evdev(usage: u32) -> Option<u32> {
    match usage {
        1 => Some(BTN_LEFT),
        2 => Some(BTN_RIGHT),
        3 => Some(BTN_MIDDLE),
        4 => Some(BTN_SIDE),
        5 => Some(BTN_EXTRA),
        6 => Some(BTN_FORWARD),
        7 => Some(BTN_BACK),
        8 => Some(BTN_TASK),
        _ => None,
    }
}

/// Map a HID Keyboard/Keypad usage to its evdev `KEY_*` code.
///
/// Covers the usages a US/ISO 105-key keyboard (or a combo receiver exposing
/// one) can emit, including the modifier usages 0xE0–0xE7 that the boot
/// protocol carries in its modifier bitmap.
pub fn keyboard_to_evdev(usage: u32) -> Option<u32> {
    Some(match usage {
        // ── Letters ──────────────────────────────────────────────────────────
        0x04 => 30, // A
        0x05 => 48, // B
        0x06 => 46, // C
        0x07 => 32, // D
        0x08 => 18, // E
        0x09 => 33, // F
        0x0A => 34, // G
        0x0B => 35, // H
        0x0C => 23, // I
        0x0D => 36, // J
        0x0E => 37, // K
        0x0F => 38, // L
        0x10 => 50, // M
        0x11 => 49, // N
        0x12 => 24, // O
        0x13 => 25, // P
        0x14 => 16, // Q
        0x15 => 19, // R
        0x16 => 31, // S
        0x17 => 20, // T
        0x18 => 22, // U
        0x19 => 47, // V
        0x1A => 17, // W
        0x1B => 45, // X
        0x1C => 21, // Y
        0x1D => 44, // Z
        // ── Number row ───────────────────────────────────────────────────────
        0x1E => 2,  // 1 !
        0x1F => 3,  // 2 @
        0x20 => 4,  // 3 #
        0x21 => 5,  // 4 $
        0x22 => 6,  // 5 %
        0x23 => 7,  // 6 ^
        0x24 => 8,  // 7 &
        0x25 => 9,  // 8 *
        0x26 => 10, // 9 (
        0x27 => 11, // 0 )
        // ── Editing / whitespace ─────────────────────────────────────────────
        0x28 => 28, // Enter
        0x29 => 1,  // Escape
        0x2A => 14, // Delete (Backspace)
        0x2B => 15, // Tab
        0x2C => 57, // Spacebar
        0x2D => 12, // - _
        0x2E => 13, // = +
        0x2F => 26, // [ {
        0x30 => 27, // ] }
        0x31 => 43, // \ |
        0x32 => 86, // Non-US # ~
        0x33 => 39, // ; :
        0x34 => 40, // ' "
        0x35 => 41, // ` ~
        0x36 => 51, // , <
        0x37 => 52, // . >
        0x38 => 53, // / ?
        0x39 => 58, // Caps Lock
        // ── Function row ─────────────────────────────────────────────────────
        0x3A => 59,  // F1
        0x3B => 60,  // F2
        0x3C => 61,  // F3
        0x3D => 62,  // F4
        0x3E => 63,  // F5
        0x3F => 64,  // F6
        0x40 => 65,  // F7
        0x41 => 66,  // F8
        0x42 => 67,  // F9
        0x43 => 68,  // F10
        0x44 => 87,  // F11
        0x45 => 88,  // F12
        0x46 => 99,  // PrintScreen
        0x47 => 70,  // Scroll Lock
        0x48 => 119, // Pause
        0x49 => 110, // Insert
        0x4A => 102, // Home
        0x4B => 104, // PageUp
        0x4C => 111, // Delete Forward
        0x4D => 107, // End
        0x4E => 109, // PageDown
        0x4F => 106, // Right Arrow
        0x50 => 105, // Left Arrow
        0x51 => 108, // Down Arrow
        0x52 => 103, // Up Arrow
        // ── Keypad ───────────────────────────────────────────────────────────
        0x53 => 69,  // Num Lock
        0x54 => 98,  // KP /
        0x55 => 55,  // KP *
        0x56 => 74,  // KP -
        0x57 => 78,  // KP +
        0x58 => 96,  // KP Enter
        0x59 => 79,  // KP 1
        0x5A => 80,  // KP 2
        0x5B => 81,  // KP 3
        0x5C => 75,  // KP 4
        0x5D => 76,  // KP 5
        0x5E => 77,  // KP 6
        0x5F => 71,  // KP 7
        0x60 => 72,  // KP 8
        0x61 => 73,  // KP 9
        0x62 => 82,  // KP 0
        0x63 => 83,  // KP .
        0x64 => 86,  // Non-US \ |
        0x65 => 127, // Application / Compose
        0x66 => 116, // Power
        0x67 => 117, // KP =
        0x68 => 183, // F13
        0x69 => 184, // F14
        0x6A => 185, // F15
        0x6B => 186, // F16
        0x6C => 187, // F17
        0x6D => 188, // F18
        0x6E => 189, // F19
        0x6F => 190, // F20
        0x70 => 191, // F21
        0x71 => 192, // F22
        0x72 => 193, // F23
        0x73 => 194, // F24
        0x74 => 134, // Execute
        0x75 => 138, // Help
        0x76 => 139, // Menu
        0x77 => 133, // Select
        0x78 => 128, // Stop
        0x79 => 129, // Again
        0x7A => 131, // Undo
        0x7B => 137, // Cut
        0x7C => 133, // Copy
        0x7D => 135, // Paste
        0x7E => 136, // Find
        0x7F => 113, // Mute
        0x80 => 115, // Volume Up
        0x81 => 114, // Volume Down
        0x85 => 121, // KP ,
        0x87 => 89,  // Ro
        0x88 => 90,  // Katakana
        0x89 => 91,  // Hiragana
        0x8A => 92,  // Henkan
        0x8B => 93,  // Katakana/Hiragana
        0x8C => 94,  // Muhenkan
        0x8D => 95,  // KP JP Comma
        0x90 => 122, // Hangeul
        0x91 => 123, // Hanja
        0x92 => 94,  // Katakana (alt)
        0x93 => 124, // Yen
        0x94 => 125, // Left Meta (alt encoding)
        0x95 => 126, // Right Meta (alt encoding)
        0xA5 => 240, // Unknown
        0xB5 => 144, // File
        0xB6 => 145, // Send File
        0xB7 => 146, // Delete File
        0xC0 => 148, // Prog1
        0xC1 => 149, // Prog2
        0xC4 => 182, // Redo
        0xE0 => 29,  // Left Ctrl
        0xE1 => 42,  // Left Shift
        0xE2 => 56,  // Left Alt
        0xE3 => 125, // Left GUI
        0xE4 => 97,  // Right Ctrl
        0xE5 => 54,  // Right Shift
        0xE6 => 100, // Right Alt
        0xE7 => 126, // Right GUI
        _ => return None,
    })
}

/// Translate a decoded HID field into a `(evdev_code, is_mouse_button)` pair.
///
/// `is_mouse_button` steers the input service down its pointer path instead of
/// its keyboard path (`code >= BTN_LEFT` there has the same meaning).
pub fn hid_field_to_evdev(page: u32, usage: u32) -> Option<(u32, bool)> {
    match page {
        PAGE_KEYBOARD => keyboard_to_evdev(usage).map(|c| (c, false)),
        PAGE_BUTTON => button_to_evdev(usage).map(|c| (c, true)),
        _ => None,
    }
}

// ─── Boot-protocol modifier bitmap (HID 1.11 §8.3, Appendix B.1) ──────────────
//
// Bit n set in report byte 0 means the usage (0xE0 + n) is held. The order is
// Ctrl, Shift, Alt, GUI — left then right — and is independent of the report
// descriptor, which is why boot protocol needs no descriptor parse.
pub const BOOT_MODIFIER_USAGES: [u32; 8] = [0xE0, 0xE1, 0xE2, 0xE3, 0xE4, 0xE5, 0xE6, 0xE7];

/// Expand a boot-protocol modifier byte into its held evdev codes.
pub fn boot_modifiers(mask: u8) -> impl Iterator<Item = u32> {
    BOOT_MODIFIER_USAGES
        .iter()
        .enumerate()
        .filter(move |(bit, _)| mask & (1 << bit) != 0)
        .filter_map(|(_, usage)| keyboard_to_evdev(*usage))
}
