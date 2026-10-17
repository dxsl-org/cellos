//! Standard PC PS/2 (i8042) Keyboard and Mouse Driver for x86_64.
//!
//! Polls the i8042 controller (status port 0x64, data port 0x60) for keyboard
//! scancodes (Set 1) and PS/2 auxiliary mouse 3-byte packets.
//! Translates keyboard to ASCII and control characters, and emits mouse
//! movement and button state changes.

use crate::sync::Spinlock;

const DATA_PORT: u16 = 0x60;
const STATUS_PORT: u16 = 0x64;

const STATUS_OUTPUT_FULL: u8 = 0x01;
const STATUS_INPUT_FULL: u8 = 0x02;
const STATUS_MOUSE_DATA: u8 = 0x20;

#[derive(Clone, Copy, Debug)]
pub enum Ps2MouseEvent {
    Button { button: u32, pressed: bool },
    Move { dx: i32, dy: i32 },
}

struct Ps2State {
    shift: bool,
    caps_lock: bool,
    ctrl: bool,
    alt: bool,
    extended: bool,
    buf: [u8; 8],
    buf_len: usize,
    buf_pos: usize,

    // Mouse packet state
    mouse_buf: [u8; 3],
    mouse_idx: usize,
    mouse_buttons: u8,
    mouse_events: [Option<Ps2MouseEvent>; 8],
    mouse_event_head: usize,
    mouse_event_tail: usize,
}

static STATE: Spinlock<Ps2State> = Spinlock::new(Ps2State {
    shift: false,
    caps_lock: false,
    ctrl: false,
    alt: false,
    extended: false,
    buf: [0; 8],
    buf_len: 0,
    buf_pos: 0,
    mouse_buf: [0; 3],
    mouse_idx: 0,
    mouse_buttons: 0,
    mouse_events: [None; 8],
    mouse_event_head: 0,
    mouse_event_tail: 0,
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

#[inline]
fn outb(port: u16, val: u8) {
    // SAFETY: writing I/O port on x86 is a Ring 0 instruction without memory aliasing.
    unsafe {
        core::arch::asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack));
    }
}

fn wait_write() {
    for _ in 0..10_000 {
        if (inb(STATUS_PORT) & STATUS_INPUT_FULL) == 0 {
            break;
        }
    }
}

fn wait_read() {
    for _ in 0..10_000 {
        if (inb(STATUS_PORT) & STATUS_OUTPUT_FULL) != 0 {
            break;
        }
    }
}

pub fn init() {
    // 1. Enable auxiliary mouse port on i8042
    wait_write();
    outb(STATUS_PORT, 0xA8);

    // 2. Read command byte
    wait_write();
    outb(STATUS_PORT, 0x20);
    wait_read();
    let mut cmd = inb(DATA_PORT);

    // 3. Clear mouse disable bit (bit 5) and enable mouse interrupt/reporting (bit 1)
    cmd = (cmd | 0x02) & !0x20;
    wait_write();
    outb(STATUS_PORT, 0x60);
    wait_write();
    outb(DATA_PORT, cmd);

    // 4. Send command 0xF4 (Enable Data Reporting) to mouse device
    wait_write();
    outb(STATUS_PORT, 0xD4);
    wait_write();
    outb(DATA_PORT, 0xF4);
    wait_read();
    let _ = inb(DATA_PORT); // ACK
}

fn push_mouse_event(state: &mut Ps2State, event: Ps2MouseEvent) {
    let next_tail = (state.mouse_event_tail + 1) % state.mouse_events.len();
    if next_tail != state.mouse_event_head {
        state.mouse_events[state.mouse_event_tail] = Some(event);
        state.mouse_event_tail = next_tail;
    }
}

pub fn poll_mouse() -> Option<Ps2MouseEvent> {
    let mut state = STATE.lock();
    let head = state.mouse_event_head;
    if head != state.mouse_event_tail {
        let ev = state.mouse_events[head].take();
        state.mouse_event_head = (head + 1) % state.mouse_events.len();
        return ev;
    }
    None
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

    // If output is from PS/2 mouse (AUX), process mouse packet
    if (status & STATUS_MOUSE_DATA) != 0 {
        let byte = inb(DATA_PORT);
        if state.mouse_idx == 0 && (byte & 0x08) == 0 {
            // Out of sync: bit 3 must always be 1 in byte 0 of standard PS/2 packet
            return None;
        }
        let idx = state.mouse_idx;
        state.mouse_buf[idx] = byte;
        state.mouse_idx = idx + 1;
        if state.mouse_idx == 3 {
            state.mouse_idx = 0;
            let b0 = state.mouse_buf[0];
            let b1 = state.mouse_buf[1];
            let b2 = state.mouse_buf[2];

            let dx = if (b0 & 0x10) != 0 {
                (b1 as i32) - 256
            } else {
                b1 as i32
            };
            let raw_dy = if (b0 & 0x20) != 0 {
                (b2 as i32) - 256
            } else {
                b2 as i32
            };
            let dy = -raw_dy;

            let old_buttons = state.mouse_buttons;
            let new_buttons = b0 & 0x07;
            state.mouse_buttons = new_buttons;

            // BTN_LEFT = 0x110
            if (new_buttons & 1) != (old_buttons & 1) {
                push_mouse_event(&mut state, Ps2MouseEvent::Button {
                    button: 0x110,
                    pressed: (new_buttons & 1) != 0,
                });
            }
            // BTN_RIGHT = 0x111
            if (new_buttons & 2) != (old_buttons & 2) {
                push_mouse_event(&mut state, Ps2MouseEvent::Button {
                    button: 0x111,
                    pressed: (new_buttons & 2) != 0,
                });
            }
            // BTN_MIDDLE = 0x112
            if (new_buttons & 4) != (old_buttons & 4) {
                push_mouse_event(&mut state, Ps2MouseEvent::Button {
                    button: 0x112,
                    pressed: (new_buttons & 4) != 0,
                });
            }
            if dx != 0 || dy != 0 {
                push_mouse_event(&mut state, Ps2MouseEvent::Move { dx, dy });
            }
        }
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
        if scancode == 0x1D {
            state.ctrl = true;
            return None;
        }
        if scancode == 0x9D {
            state.ctrl = false;
            return None;
        }
        if scancode == 0x38 {
            state.alt = true;
            return None;
        }
        if scancode == 0xB8 {
            state.alt = false;
            return None;
        }
        // Extended keys (arrows, Home/End, PgUp/PgDn)
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
            0x47 => { // Home -> \x1b[H
                state.buf = [b'\x1b', b'[', b'H', 0, 0, 0, 0, 0];
                state.buf_len = 3;
                state.buf_pos = 1;
                Some(b'\x1b')
            }
            0x4F => { // End -> \x1b[F
                state.buf = [b'\x1b', b'[', b'F', 0, 0, 0, 0, 0];
                state.buf_len = 3;
                state.buf_pos = 1;
                Some(b'\x1b')
            }
            0x49 => { // PageUp -> \x1b[5~
                state.buf = [b'\x1b', b'[', b'5', b'~', 0, 0, 0, 0];
                state.buf_len = 4;
                state.buf_pos = 1;
                Some(b'\x1b')
            }
            0x51 => { // PageDown -> \x1b[6~
                state.buf = [b'\x1b', b'[', b'6', b'~', 0, 0, 0, 0];
                state.buf_len = 4;
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
        } else if released == 0x1D {
            state.ctrl = false;
        } else if released == 0x38 {
            state.alt = false;
        }
        return None;
    }

    // Key presses
    match scancode {
        0x2A | 0x36 => {
            state.shift = true;
            None
        }
        0x1D => {
            state.ctrl = true;
            None
        }
        0x38 => {
            state.alt = true;
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
        0x3B => { // F1
            state.buf = [b'\x1b', b'[', b'1', b'1', b'~', 0, 0, 0];
            state.buf_len = 5; state.buf_pos = 1; Some(b'\x1b')
        }
        0x3C => { // F2
            state.buf = [b'\x1b', b'[', b'1', b'2', b'~', 0, 0, 0];
            state.buf_len = 5; state.buf_pos = 1; Some(b'\x1b')
        }
        0x3D => { // F3
            state.buf = [b'\x1b', b'[', b'1', b'3', b'~', 0, 0, 0];
            state.buf_len = 5; state.buf_pos = 1; Some(b'\x1b')
        }
        0x3E => { // F4
            state.buf = [b'\x1b', b'[', b'1', b'4', b'~', 0, 0, 0];
            state.buf_len = 5; state.buf_pos = 1; Some(b'\x1b')
        }
        0x3F => { // F5
            state.buf = [b'\x1b', b'[', b'1', b'5', b'~', 0, 0, 0];
            state.buf_len = 5; state.buf_pos = 1; Some(b'\x1b')
        }
        _ => {
            let ch = translate_scancode(scancode, state.shift, state.caps_lock);
            if state.ctrl {
                if let Some(c) = ch {
                    if c.is_ascii_alphabetic() {
                        return Some(c.to_ascii_uppercase() - b'A' + 1);
                    }
                }
            }
            ch
        }
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
