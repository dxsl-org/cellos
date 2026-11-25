//! The clipboard line protocol on the guest's `/dev/hvc0`, both directions:
//! one line per clipboard text, `C` + standard base64 of the UTF-8 text +
//! `\n`. Lines not starting with `C` are ignored.
//!
//! The guest is untrusted: a line is buffered up to [`MAX_LINE`] bytes and
//! anything longer is discarded through its newline, so a guest that never
//! ends a line holds a bounded buffer.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use crate::base64;

/// Largest clipboard text, either way (`wire::display::CLIPBOARD_MAX`).
pub const MAX_TEXT: usize = wire::display::CLIPBOARD_MAX;
/// Longest line kept, newline excluded: `C` plus the padded base64 of
/// [`MAX_TEXT`] bytes (87 385) with room to spare.
pub const MAX_LINE: usize = 90_000;
const TAG: u8 = b'C';

/// Collects TX bytes into lines and decodes the clipboard ones.
#[derive(Default)]
pub struct LineDecoder {
    line: Vec<u8>,
    /// The current line went over [`MAX_LINE`]: drop bytes through its `\n`.
    discarding: bool,
}

impl LineDecoder {
    pub const fn new() -> Self {
        Self {
            line: Vec::new(),
            discarding: false,
        }
    }

    /// Feed guest bytes; the newest text completed among them, if any.
    pub fn feed(&mut self, bytes: &[u8]) -> Option<String> {
        let mut newest = None;
        for &byte in bytes {
            if byte == b'\n' {
                if !core::mem::take(&mut self.discarding) {
                    newest = decode_line(&self.line).or(newest);
                }
                self.line.clear();
            } else if !self.discarding {
                if self.line.len() == MAX_LINE {
                    self.line = Vec::new();
                    self.discarding = true;
                } else {
                    self.line.push(byte);
                }
            }
        }
        newest
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

/// The text of one line (newline excluded), or `None` when it is not a
/// clipboard line, not base64, empty, over [`MAX_TEXT`] or not UTF-8. An
/// incomplete UTF-8 sequence at the end (a text cut at a byte limit) is
/// trimmed; invalid UTF-8 elsewhere drops the line.
fn decode_line(line: &[u8]) -> Option<String> {
    let encoded = line.strip_prefix(&[TAG])?;
    let mut text = base64::decode(encoded)?;
    if text.len() > MAX_TEXT {
        return None;
    }
    if let Err(error) = core::str::from_utf8(&text) {
        if error.error_len().is_some() {
            return None;
        }
        text.truncate(error.valid_up_to());
    }
    if text.is_empty() {
        return None;
    }
    String::from_utf8(text).ok()
}

/// The line carrying `text` to the guest.
pub fn encode_line(text: &str) -> Vec<u8> {
    let mut line = Vec::with_capacity(text.len().div_ceil(3) * 4 + 2);
    line.push(TAG);
    base64::encode_into(text.as_bytes(), &mut line);
    line.push(b'\n');
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn clipboard_line_round_trips_text() {
        let line = encode_line("héllo, wörld");
        assert_eq!(&line[..1], b"C");
        assert_eq!(line.last(), Some(&b'\n'));
        let mut decoder = LineDecoder::new();
        assert_eq!(decoder.feed(&line).as_deref(), Some("héllo, wörld"));
    }

    #[test]
    fn clipboard_line_split_across_tx_chunks() {
        let line = encode_line("split text");
        let mut decoder = LineDecoder::new();
        let (a, b) = line.split_at(5);
        assert_eq!(decoder.feed(a), None);
        assert_eq!(decoder.feed(b).as_deref(), Some("split text"));
    }

    #[test]
    fn clipboard_line_keeps_newest_and_ignores_other_lines() {
        let mut bytes = b"log line\nXZm9v\n".to_vec();
        bytes.extend(encode_line("first"));
        bytes.extend(encode_line("second"));
        bytes.extend(b"C!!!!\n");
        let mut decoder = LineDecoder::new();
        assert_eq!(decoder.feed(&bytes).as_deref(), Some("second"));
    }

    #[test]
    fn clipboard_line_over_long_is_discarded_through_newline() {
        let mut decoder = LineDecoder::new();
        let long = vec![b'A'; MAX_LINE + 10];
        assert_eq!(decoder.feed(b"C"), None);
        assert_eq!(decoder.feed(&long), None);
        assert_eq!(decoder.feed(b"Zm9v\n"), None);
        assert_eq!(
            decoder.feed(&encode_line("after")).as_deref(),
            Some("after")
        );
    }

    #[test]
    fn clipboard_line_max_text_fits_and_over_max_is_dropped() {
        let text = "x".repeat(MAX_TEXT);
        let mut decoder = LineDecoder::new();
        assert_eq!(decoder.feed(&encode_line(&text)), Some(text));
        let mut over = vec![TAG];
        base64::encode_into(&vec![b'x'; MAX_TEXT + 1], &mut over);
        over.push(b'\n');
        assert_eq!(decoder.feed(&over), None);
    }

    #[test]
    fn clipboard_line_trims_truncated_utf8_tail() {
        // "ab" + the first two bytes of "€" (E2 82 AC).
        let mut line = vec![TAG];
        base64::encode_into(&[b'a', b'b', 0xE2, 0x82], &mut line);
        line.push(b'\n');
        assert_eq!(LineDecoder::new().feed(&line).as_deref(), Some("ab"));
    }

    #[test]
    fn clipboard_line_drops_invalid_utf8_and_empty_text() {
        let mut decoder = LineDecoder::new();
        let mut bad = vec![TAG];
        base64::encode_into(&[b'a', 0xFF, b'b'], &mut bad);
        bad.push(b'\n');
        assert_eq!(decoder.feed(&bad), None);
        assert_eq!(decoder.feed(b"C\n"), None);
        // Only an incomplete sequence: trimmed to nothing.
        let mut cut = vec![TAG];
        base64::encode_into(&[0xE2], &mut cut);
        cut.push(b'\n');
        assert_eq!(decoder.feed(&cut), None);
    }
}
