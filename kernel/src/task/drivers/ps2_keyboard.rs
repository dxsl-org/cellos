//! Standard PC PS/2 (i8042) Keyboard Driver for x86_64.
//!
//! Polls the i8042 controller (status port 0x64, data port 0x60) for keyboard
//! scancodes (Set 1), translates them to ASCII characters or ANSI escape
//! sequences, and provides them to the console driver.

use crate::sync::Spinlock;

const DATA_PORT: u16 = 0x60;
const STATUS_PORT: u16 = 0x64;

const STATUS_OUTPUT_FULL: u8 = 0x01;
const STATUS_MOUSE_DATA: u8 = 0x20;

#[derive(Default)]
struct Ps2State {
    shift: bool,
    caps_lock: bool,
    extended: bool,
    buf: [u8; 8],
    buf_len: usize,
    buf_pos: usize,
}
static STATE: Spinlock<Ps2State> = Spinlock::new(Ps2State {
    shift: false,
    caps_lock: false,
    extended: false,
    buf: [0; 8],
    buf_len: 0,
    buf_pos: 0,
});

#[inline]
fn inb(port: u16) -> u8 {
    let val: u8;
    // SAFETY: reading I/O port on x86 is a Ring 0 instruction without memory aliasing.
    unsafe {
        core::arch::asm!("in al, dx", in("dx") port, out("al") val, options(nomem, nostack));
    }
    val
}

/// Poll the i8042 keyboard controller for an ASCII byte.
pub fn poll() -> Option<u8> {
    let mut state = STATE.lock();

    // Drain queued multi-byte sequence if any
    if state.buf_pos < state.buf_len {
        let byte = state.buf[state.buf_pos];
        state.buf_pos += 1;
        if state.buf_pos >= state.buf_len {
            state.buf_pos = 0;
            state.buf_len = 0;
        }
        return Some(byte);
    }

    let status = inb(STATUS_PORT);
    if (status & STATUS_OUTPUT_FULL) == 0 {
        return None;
    }
    // If output is from PS/2 mouse (AUX), ignore it here
    if (status & STATUS_MOUSE_DATA) != 0 {
        let _ = inb(DATA_PORT);
        return None;
    }

    let scancode = inb(DATA_PORT);

    // Extended scancode prefix
    if scancode == 0xE0 {
        state.extended = true;
        return None;
    }

    if state.extended {
        state.extended = false;
        // Extended keys (e.g. arrow keys)
        return match scancode {
            0x48 => { // Up arrow -> \x1b[A
                state.buf = [b'\x1b', b'[', b'A', 0, 0, 0, 0, 0];
                state.buf_len = 3;
                state.buf_pos = 1;
                Some(b'\x1b')
            }
            0x50 => { // Down arrow -> \x1b[B
                state.buf = [b'\x1b', b'[', b'B', 0, 0, 0, 0, 0];
                state.buf_len = 3;
                state.buf_pos = 1;
                Some(b'\x1b')
            }
            0x4D => { // Right arrow -> \x1b[C
                state.buf = [b'\x1b', b'[', b'C', 0, 0, 0, 0, 0];
                state.buf_len = 3;
                state.buf_pos = 1;
                Some(b'\x1b')
            }
            0x4B => { // Left arrow -> \x1b[D
                state.buf = [b'\x1b', b'[', b'D', 0, 0, 0, 0, 0];
                state.buf_len = 3;
                state.buf_pos = 1;
                Some(b'\x1b')
            }
            _ => None,
        };
    }

    // Key releases (bit 7 set)
    if (scancode & 0x80) != 0 {
        let released = scancode & 0x7F;
        if released == 0x2A || released == 0x36 {
            state.shift = false;
        }
        return None;
    }

    // Key presses
    match scancode {
        0x2A | 0x36 => {
            state.shift = true;
            None
        }
        0x3A => {
            state.caps_lock = !state.caps_lock;
            None
        }
        0x1C => Some(b'\n'), // Enter
        0x0E => Some(0x08),   // Backspace
        0x0F => Some(b'\t'), // Tab
        0x39 => Some(b' '),  // Space
        0x01 => Some(0x1B),   // Escape
        _ => translate_scancode(scancode, state.shift, state.caps_lock),
    }
}

fn translate_scancode(scancode: u8, shift: bool, caps: bool) -> Option<u8> {
    let upper = shift ^ caps;
    match scancode {
        // Numbers and symbols row
        0x02 => Some(if shift { b'!' } else { b'1' }),
        0x03 => Some(if shift { b'@' } else { b'2' }),
        0x04 => Some(if shift { b'#' } else { b'3' }),
        0x05 => Some(if shift { b'$' } else { b'4' }),
        0x06 => Some(if shift { b'%' } else { b'5' }),
        0x07 => Some(if shift { b'^' } else { b'6' }),
        0x08 => Some(if shift { b'&' } else { b'7' }),
        0x09 => Some(if shift { b'*' } else { b'8' }),
        0x0A => Some(if shift { b'(' } else { b'9' }),
        0x0B => Some(if shift { b')' } else { b'0' }),
        0x0C => Some(if shift { b'_' } else { b'-' }),
        0x0D => Some(if shift { b'+' } else { b'=' }),

        // Letters
        0x1E => Some(if upper { b'A' } else { b'a' }),
        0x30 => Some(if upper { b'B' } else { b'b' }),
        0x2E => Some(if upper { b'C' } else { b'c' }),
        0x20 => Some(if upper { b'D' } else { b'd' }),
        0x12 => Some(if upper { b'E' } else { b'e' }),
        0x21 => Some(if upper { b'F' } else { b'f' }),
        0x22 => Some(if upper { b'G' } else { b'g' }),
        0x23 => Some(if upper { b'H' } else { b'h' }),
        0x17 => Some(if upper { b'I' } else { b'i' }),
        0x24 => Some(if upper { b'J' } else { b'j' }),
        0x25 => Some(if upper { b'K' } else { b'k' }),
        0x26 => Some(if upper { b'L' } else { b'l' }),
        0x32 => Some(if upper { b'M' } else { b'm' }),
        0x31 => Some(if upper { b'N' } else { b'n' }),
        0x18 => Some(if upper { b'O' } else { b'o' }),
        0x19 => Some(if upper { b'P' } else { b'p' }),
        0x10 => Some(if upper { b'Q' } else { b'q' }),
        0x13 => Some(if upper { b'R' } else { b'r' }),
        0x1F => Some(if upper { b'S' } else { b's' }),
        0x14 => Some(if upper { b'T' } else { b't' }),
        0x16 => Some(if upper { b'U' } else { b'u' }),
        0x2F => Some(if upper { b'V' } else { b'v' }),
        0x11 => Some(if upper { b'W' } else { b'w' }),
        0x2D => Some(if upper { b'X' } else { b'x' }),
        0x15 => Some(if upper { b'Y' } else { b'y' }),
        0x2C => Some(if upper { b'Z' } else { b'z' }),

        // Punctuation
        0x1A => Some(if shift { b'{' } else { b'[' }),
        0x1B => Some(if shift { b'}' } else { b']' }),
        0x27 => Some(if shift { b':' } else { b';' }),
        0x28 => Some(if shift { b'"' } else { b'\'' }),
        0x29 => Some(if shift { b'~' } else { b'`' }),
        0x2B => Some(if shift { b'|' } else { b'\\' }),
        0x33 => Some(if shift { b'<' } else { b',' }),
        0x34 => Some(if shift { b'>' } else { b'.' }),
        0x35 => Some(if shift { b'?' } else { b'/' }),

        _ => None,
    }
}
