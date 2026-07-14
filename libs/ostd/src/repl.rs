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

    /// Access entry by 0-based index (0 = oldest).
    pub fn get(&self, idx: usize) -> Option<&str> {
        self.entries.get(idx).map(String::as_str)
    }
}

/// Minimal interactive line editor.
///
/// Reads from `fd 0` (stdin) one byte at a time, handling:
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
        }
    }

    /// Display `prompt` and read one complete line.
    pub fn read_line(&mut self, prompt: &str) -> ReadResult {
        crate::io::print(prompt);
        self.len = 0;
        self.hist_idx = self.history.len();
        self.esc_state = 0;

        loop {
            let mut c = [0u8; 1];
            match syscall::sys_read(0, &mut c) {
                Ok(1) => {
                    if let Some(result) = self.process_byte(c[0]) {
                        return result;
                    }
                }
                _ => return ReadResult::Eof,
            }
        }
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

impl Default for Repl {
    fn default() -> Self {
        Self::new()
    }
}
