//! Shared readline / REPL state machine for interactive cells.
//!
//! Provides line editing (Backspace, Ctrl+C/D, arrow history nav)
//! and a ring-buffer command history with configurable capacity.

extern crate alloc;

use crate::syscall;
use alloc::borrow::ToOwned;
use alloc::collections::VecDeque;
use alloc::string::String;

/// Maximum bytes in a single input line.
const MAX_LINE: usize = 4096;
/// Capacity of the history ring buffer (oldest dropped when full).
const HISTORY_CAP: usize = 500;

/// Result of a single `Repl::read_line` call.
#[derive(Debug)]
pub enum ReadResult {
    /// The user typed a complete line (may be empty).  Newline is stripped.
    Line(String),
    /// Ctrl+C pressed — discard current input, return empty line indicator.
    Interrupted,
    /// Ctrl+D / EOF — caller should exit the REPL.
    Eof,
}

/// Ring-buffer command history.
#[derive(Default)]
pub struct History {
    entries: VecDeque<String>,
}

impl History {
    pub fn new() -> Self {
        Self {
            entries: VecDeque::with_capacity(HISTORY_CAP),
        }
    }

    /// Push a line (de-duplicates consecutive identical entries).
    pub fn push(&mut self, line: &str) {
        if line.is_empty() {
            return;
        }
        if self.entries.back().map(|s| s.as_str()) == Some(line) {
            return;
        }
        if self.entries.len() >= HISTORY_CAP {
            self.entries.pop_front();
        }
        self.entries.push_back(String::from(line));
    }

    /// Number of history entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when there is no history yet.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Access entry by 0-based index (0 = oldest).
    pub fn get(&self, idx: usize) -> Option<&str> {
        self.entries.get(idx).map(String::as_str)
    }
}

/// Minimal interactive line editor.
///
/// Reads console input one byte at a time (see [`Repl::read_line`] for the
/// sources), handling:
/// - Printable ASCII: echo + append to buffer
/// - Backspace / DEL: erase last character
/// - Enter / CR: return the completed line
/// - Ctrl+C (0x03): return `Interrupted`
/// - Ctrl+D (0x04) / EOF: return `Eof`
/// - ESC `[` `A` (up-arrow): navigate to previous history entry
/// - ESC `[` `B` (down-arrow): navigate to next history entry
pub struct Repl {
    pub history: History,
    buf: [u8; MAX_LINE],
    len: usize,
    hist_idx: usize,
    esc_state: u8, // 0=normal 1=ESC 2=ESC+[
    /// Bytes decoded from one input-service key event, not yet processed.
    pending: [u8; 4],
    pending_pos: usize,
    pending_len: usize,
    /// Input-service TID this REPL last claimed console focus from (0 = none).
    focused_on: usize,
}

impl Repl {
    pub fn new() -> Self {
        let history = History::new();
        let hist_idx = 0;
        Self {
            history,
            buf: [0; MAX_LINE],
            len: 0,
            hist_idx,
            esc_state: 0,
            pending: [0; 4],
            pending_pos: 0,
            pending_len: 0,
            focused_on: 0,
        }
    }

    /// Display `prompt` and read one complete line.
    ///
    /// Input sources, the same as [`crate::io::Stdin::read_line`]:
    /// - `fd 0`: a session foreground child's stdin stream (the kernel blocks
    ///   until bytes arrive), or the legacy console ring, which is
    ///   non-blocking — `Ok(0)` there means "nothing yet", not end of input.
    /// - the input service: a shell outside a terminal session releases
    ///   console focus while its foreground child runs, and serial bytes
    ///   reach that child only as key events once it claims focus. Needs the
    ///   `Send`, `RecvTimeout` and `LookupServiceBound` syscalls.
    ///
    /// An `fd 0` error (endpoint gone, `Read` not granted) ends input: `Eof`.
    pub fn read_line(&mut self, prompt: &str) -> ReadResult {
        crate::io::print(prompt);
        self.len = 0;
        self.hist_idx = self.history.len();
        self.esc_state = 0;

        loop {
            let Some(byte) = self.next_byte() else {
                return ReadResult::Eof;
            };
            if let Some(result) = self.process_byte(byte) {
                return result;
            }
        }
    }

    /// Next console byte; `None` once `fd 0` reports an error.
    fn next_byte(&mut self) -> Option<u8> {
        loop {
            if self.pending_pos < self.pending_len {
                self.pending_pos += 1;
                return Some(self.pending[self.pending_pos - 1]);
            }
            let mut c = [0u8; 1];
            match syscall::sys_read(0, &mut c) {
                Ok(1) => return Some(c[0]),
                Ok(0) => {}
                _ => return None,
            }
            if !self.poll_input_service() {
                crate::task::yield_now();
            }
        }
    }

    /// Wait briefly for one key event from the input service and queue its
    /// bytes. Returns `false` when no input service is registered.
    fn poll_input_service(&mut self) -> bool {
        use wire::input::{decode_event, InputEvent, INPUT_EVENT_OPCODE};
        // Short enough that console bytes on fd 0 are still picked up promptly.
        const TIMEOUT_TICKS: u64 = 2;
        let Some(input_tid) = crate::service::provider_tid(abi::syscall::service::INPUT) else {
            return false;
        };
        // Claim focus once per input-service incarnation, as Stdin does:
        // re-claiming on every poll would race the service's dispatch.
        if self.focused_on != input_tid {
            if !crate::input::request_focus() {
                return true;
            }
            self.focused_on = input_tid;
        }
        let mut frame = [0u8; wire::ipc::IPC_BUF_SIZE];
        if let syscall::SyscallResult::Ok(sender) =
            syscall::sys_recv_timeout(0, &mut frame, TIMEOUT_TICKS)
        {
            if sender == input_tid && frame[0] == INPUT_EVENT_OPCODE {
                if let Some(InputEvent::Key(key)) = decode_event(&frame[1..]) {
                    self.pending_len = key_bytes(&key, &mut self.pending);
                    self.pending_pos = 0;
                }
            }
        }
        true
    }

    /// Process one byte.  Returns `Some(result)` if a line is complete.
    fn process_byte(&mut self, byte: u8) -> Option<ReadResult> {
        match self.esc_state {
            1 => {
                self.esc_state = if byte == b'[' { 2 } else { 0 };
                return None;
            }
            2 => {
                self.esc_state = 0;
                match byte {
                    b'A' => self.history_prev(),
                    b'B' => self.history_next(),
                    _ => {}
                }
                return None;
            }
            _ => {}
        }

        match byte {
            0x1B => {
                self.esc_state = 1;
                None
            }
            0x03 => {
                // Ctrl+C
                crate::io::println("^C");
                Some(ReadResult::Interrupted)
            }
            0x04 if self.len == 0 => {
                // Ctrl+D on empty line → EOF
                crate::io::println("");
                Some(ReadResult::Eof)
            }
            b'\r' | b'\n' => {
                crate::io::println("");
                let s = core::str::from_utf8(&self.buf[..self.len])
                    .unwrap_or("")
                    .to_owned();
                self.history.push(&s);
                Some(ReadResult::Line(s))
            }
            // Backspace or DEL
            8 | 127 => {
                if self.len > 0 {
                    self.len -= 1;
                    crate::io::print("\x08 \x08");
                }
                None
            }
            // Ctrl+U — clear line
            0x15 => {
                self.clear_display();
                self.len = 0;
                None
            }
            // Printable ASCII
            0x20..=0x7E if self.len < MAX_LINE - 1 => {
                self.buf[self.len] = byte;
                self.len += 1;
                // Echo the character — single-byte slice lives for the duration of this block.
                let echo_buf = [byte];
                // SAFETY: byte is a printable ASCII character (0x20..=0x7E), always valid UTF-8.
                let ch = unsafe { core::str::from_utf8_unchecked(&echo_buf) };
                crate::io::print(ch);
                None
            }
            _ => None,
        }
    }

    fn clear_display(&self) {
        for _ in 0..self.len {
            crate::io::print("\x08 \x08");
        }
    }

    fn history_prev(&mut self) {
        if self.hist_idx == 0 {
            return;
        }
        self.hist_idx -= 1;
        self.load_history_entry();
    }

    fn history_next(&mut self) {
        if self.hist_idx >= self.history.len() {
            return;
        }
        self.hist_idx += 1;
        self.load_history_entry();
    }

    fn load_history_entry(&mut self) {
        self.clear_display();
        self.len = 0;
        if let Some(entry) = self.history.get(self.hist_idx) {
            let bytes = entry.as_bytes();
            let n = bytes.len().min(MAX_LINE - 1);
            self.buf[..n].copy_from_slice(&bytes[..n]);
            self.len = n;
            crate::io::print(entry);
        }
    }
}

/// The bytes a terminal would send for `key`, so serial bytes relayed as key
/// events (the input service maps C0 controls to `Escape`/`Return`/`Backspace`
/// and passes the rest as `Printable` with the raw byte) and physical keys
/// feed the same byte state machine. Returns how many bytes were written.
fn key_bytes(key: &wire::input::KeyEvent, out: &mut [u8; 4]) -> usize {
    use wire::input::{KeyState, KeySym, Modifiers};
    if key.state == KeyState::Released {
        return 0;
    }
    let fixed: &[u8] = match key.keysym {
        KeySym::Escape => b"\x1b",
        KeySym::Return => b"\r",
        KeySym::Backspace => b"\x7f",
        KeySym::Up => b"\x1b[A",
        KeySym::Down => b"\x1b[B",
        KeySym::Printable => {
            let Some(ch) = key.char() else {
                return 0;
            };
            if key.modifiers.contains(Modifiers::CTRL) && ch.is_ascii_alphabetic() {
                out[0] = ch.to_ascii_uppercase() as u8 & 0x1f;
                return 1;
            }
            return ch.encode_utf8(out).len();
        }
        _ => return 0,
    };
    out[..fixed.len()].copy_from_slice(fixed);
    fixed.len()
}

impl Default for Repl {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::key_bytes;
    use wire::input::{KeyEvent, KeyState, KeySym, Modifiers};

    fn key(keysym: KeySym, character: u32, modifiers: Modifiers, state: KeyState) -> KeyEvent {
        KeyEvent {
            timestamp_ticks: 0,
            scancode: 0,
            keysym,
            character,
            modifiers,
            state,
            _pad: [0; 2],
        }
    }

    fn bytes(event: KeyEvent) -> alloc::vec::Vec<u8> {
        let mut out = [0u8; 4];
        let n = key_bytes(&event, &mut out);
        out[..n].to_vec()
    }

    #[test]
    fn relayed_serial_bytes_round_trip() {
        // The input service relays a UART byte as Printable with the raw code,
        // including C0 controls it has no KeySym for (Ctrl+C, Ctrl+D, Ctrl+U).
        for code in [b'a', b'(', 0x03, 0x04, 0x15] {
            let event = key(
                KeySym::Printable,
                code as u32,
                Modifiers(0),
                KeyState::Pressed,
            );
            assert_eq!(bytes(event), [code]);
        }
        let none = Modifiers(0);
        assert_eq!(
            bytes(key(KeySym::Return, 0x0d, none, KeyState::Pressed)),
            b"\r"
        );
        assert_eq!(
            bytes(key(KeySym::Backspace, 0x08, none, KeyState::Pressed)),
            [0x7f]
        );
        assert_eq!(
            bytes(key(KeySym::Escape, 0, none, KeyState::Pressed)),
            [0x1b]
        );
    }

    #[test]
    fn physical_keys_become_terminal_sequences() {
        let none = Modifiers(0);
        assert_eq!(
            bytes(key(KeySym::Up, 0, none, KeyState::Pressed)),
            b"\x1b[A"
        );
        assert_eq!(
            bytes(key(KeySym::Down, 0, none, KeyState::Repeated)),
            b"\x1b[B"
        );
        let ctrl_d = key(
            KeySym::Printable,
            'd' as u32,
            Modifiers::CTRL,
            KeyState::Pressed,
        );
        assert_eq!(bytes(ctrl_d), [0x04]);
        let euro = key(KeySym::Printable, '€' as u32, none, KeyState::Pressed);
        assert_eq!(bytes(euro), "€".as_bytes());
    }

    #[test]
    fn releases_and_unmapped_keys_produce_nothing() {
        let none = Modifiers(0);
        assert!(bytes(key(KeySym::Printable, 'a' as u32, none, KeyState::Released)).is_empty());
        assert!(bytes(key(KeySym::F1, 0, none, KeyState::Pressed)).is_empty());
        assert!(bytes(key(KeySym::Printable, 0, none, KeyState::Pressed)).is_empty());
    }
}
