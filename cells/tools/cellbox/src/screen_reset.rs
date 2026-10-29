//! `cellos-vt1` soft reset after a relayed child (Phase 05 S5-2).
//!
//! cellbox cannot observe a child's raw-mode lease (no ABI exposes it), so it follows
//! the screen modes the child's own output switched instead. Wherever the shell relays
//! a child's bytes to the terminal (the serial foreground drain and the external
//! pipeline pump), one `ModeTracker` per relayed stream follows DECSET/DECRST of
//! exactly `?1049` (alternate screen), `?25` (cursor visibility) and `?2004`
//! (bracketed paste). When the child or pipeline has ended and any of them was left
//! away from the canonical baseline (primary screen, cursor visible, paste off), the
//! shell writes `SOFT_RESET` once to its own stdout. An ordinary command, or a TUI
//! that restored its modes, produces no bytes; builtin output is never tracked.
//!
//! Parsing follows the spec §4 bounds: a control longer than `MAX_CONTROL` bytes or
//! with more than `MAX_PARAMS` parameters is consumed and ignored. ESC always starts a
//! new escape (also inside an unfinished control or an OSC string, which is the
//! conservative reading: at worst one more harmless reset), CAN/SUB abort, other C0
//! controls inside a CSI execute in place, a non-ASCII byte abandons the control.
//! State is fixed-size, survives chunk edges and nothing is allocated.

/// `ESC[?1049l ESC[?25h ESC[?2004l ESC[0m`: primary screen, cursor visible, bracketed
/// paste off, attributes reset.
pub const SOFT_RESET: &[u8] = b"\x1b[?1049l\x1b[?25h\x1b[?2004l\x1b[0m";

/// Most parameters one control may carry.
const MAX_PARAMS: usize = 16;
/// Most bytes one control may span, ESC and final byte included.
const MAX_CONTROL: usize = 256;

const ESC: u8 = 0x1b;
const CAN: u8 = 0x18;
const SUB: u8 = 0x1a;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Ground,
    /// After ESC.
    Escape,
    /// After `ESC [`.
    CsiEntry,
    /// Collecting the parameters of `ESC [ ?`.
    Private,
    /// Inside a control that cannot be a tracked DECSET/DECRST: consumed to its final byte.
    Ignore,
}

/// The three tracked modes of one relayed byte stream.
#[derive(Clone, Copy, Debug)]
pub struct ModeTracker {
    state: State,
    params: [u16; MAX_PARAMS],
    /// Parameters in use; `MAX_PARAMS + 1` marks an overflowing control.
    count: usize,
    /// Bytes of the current control, saturating just past `MAX_CONTROL`.
    len: usize,
    alternate: bool,
    cursor_hidden: bool,
    paste: bool,
}

impl ModeTracker {
    pub const fn new() -> Self {
        Self {
            state: State::Ground,
            params: [0; MAX_PARAMS],
            count: 0,
            len: 0,
            alternate: false,
            cursor_hidden: false,
            paste: false,
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.step(byte);
        }
    }

    /// True when the stream left any tracked mode away from the canonical baseline.
    pub fn dirty(&self) -> bool {
        self.alternate || self.cursor_hidden || self.paste
    }

    fn step(&mut self, byte: u8) {
        if byte == ESC {
            self.state = State::Escape;
            self.len = 1;
            return;
        }
        if self.state == State::Ground {
            return;
        }
        if byte == CAN || byte == SUB {
            self.state = State::Ground;
            return;
        }
        if self.len <= MAX_CONTROL {
            self.len += 1;
        }
        self.state = match self.state {
            State::Ground => State::Ground,
            State::Escape if byte == b'[' => State::CsiEntry,
            State::Escape => State::Ground,
            State::CsiEntry => match byte {
                b'?' => {
                    self.params = [0; MAX_PARAMS];
                    self.count = 1;
                    State::Private
                }
                0x40..=0x7e | 0x80..=0xff => State::Ground,
                0x20..=0x3f => State::Ignore,
                _ => State::CsiEntry,
            },
            State::Private => match byte {
                b'0'..=b'9' => {
                    if self.count <= MAX_PARAMS {
                        let param = &mut self.params[self.count - 1];
                        *param = param.saturating_mul(10).saturating_add(u16::from(byte - b'0'));
                    }
                    State::Private
                }
                b';' => {
                    self.count = (self.count + 1).min(MAX_PARAMS + 1);
                    State::Private
                }
                b'h' | b'l' => {
                    if self.count <= MAX_PARAMS && self.len <= MAX_CONTROL {
                        self.apply(byte == b'h');
                    }
                    State::Ground
                }
                0x40..=0x7e | 0x80..=0xff => State::Ground,
                0x20..=0x3f => State::Ignore,
                _ => State::Private,
            },
            State::Ignore => match byte {
                0x40..=0x7e | 0x80..=0xff => State::Ground,
                _ => State::Ignore,
            },
        };
    }

    fn apply(&mut self, set: bool) {
        for &param in &self.params[..self.count] {
            match param {
                1049 => self.alternate = set,
                25 => self.cursor_hidden = !set,
                2004 => self.paste = set,
                _ => {}
            }
        }
    }
}

impl Default for ModeTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// One tracker per relayed stream (stdout, stderr): a switch split across the two
/// streams is never stitched together, and either stream leaving a mode on counts.
#[derive(Clone, Copy, Debug, Default)]
pub struct RelayScreen {
    streams: [ModeTracker; 2],
}

impl RelayScreen {
    pub const fn new() -> Self {
        Self { streams: [ModeTracker::new(); 2] }
    }

    /// Follow `bytes` the shell relays from a child's stdout (`diagnostic == false`)
    /// or stderr stream.
    pub fn feed(&mut self, diagnostic: bool, bytes: &[u8]) {
        self.streams[usize::from(diagnostic)].feed(bytes);
    }

    pub fn needs_reset(&self) -> bool {
        self.streams.iter().any(ModeTracker::dirty)
    }
}

#[cfg(test)]
mod tests {
    use super::{ModeTracker, RelayScreen, MAX_CONTROL, MAX_PARAMS, SOFT_RESET};
    use alloc::vec::Vec;

    const TUI_ENTER: &[u8] = b"\x1b[?1049h\x1b[?25l\x1b[?2004h";
    const TUI_LEAVE: &[u8] = b"\x1b[?2004l\x1b[?25h\x1b[?1049l";

    fn dirty_after(bytes: &[u8]) -> bool {
        let mut tracker = ModeTracker::new();
        tracker.feed(bytes);
        tracker.dirty()
    }

    fn cat(parts: &[&[u8]]) -> Vec<u8> {
        parts.concat()
    }

    #[test]
    fn soft_reset_bytes_are_exact_and_restore_every_tracked_mode() {
        assert_eq!(SOFT_RESET, b"\x1b[?1049l\x1b[?25h\x1b[?2004l\x1b[0m");
        assert_eq!(SOFT_RESET.len(), 26);
        assert!(!dirty_after(&cat(&[TUI_ENTER, b"frame", SOFT_RESET])));
    }

    #[test]
    fn tui_enter_without_restore_is_dirty_one_mode_at_a_time() {
        assert!(dirty_after(TUI_ENTER));
        assert!(dirty_after(b"\x1b[?1049h"));
        assert!(dirty_after(b"\x1b[?25l"));
        assert!(dirty_after(b"\x1b[?2004h"));
        assert!(dirty_after(&cat(&[TUI_ENTER, b"\x1b[?2004l\x1b[?25h"])), "alternate screen still on");
    }

    #[test]
    fn tui_enter_then_restore_is_clean() {
        assert!(!dirty_after(&cat(&[TUI_ENTER, b"\x1b[2J\x1b[1;1Hhello\x1b[0m", TUI_LEAVE])));
        // Baseline-preserving switches are no-ops.
        assert!(!dirty_after(b"\x1b[?1049l\x1b[?25h\x1b[?2004l"));
        // Ordinary output never dirties.
        assert!(!dirty_after(b"p03-hex len=6 hex=00ff41fe807a\n\x00\xff\xfe\x80"));
    }

    #[test]
    fn every_split_point_matches_the_unsplit_stream() {
        let enter = cat(&[b"text ", TUI_ENTER, b" more \x1b[?1049;2004l\x1b[31m"]);
        let restored = cat(&[TUI_ENTER, b"\xe2\x82\xac", TUI_LEAVE]);
        for (stream, expected) in [(enter.as_slice(), true), (restored.as_slice(), false)] {
            assert_eq!(dirty_after(stream), expected);
            for split in 0..=stream.len() {
                let mut tracker = ModeTracker::new();
                tracker.feed(&stream[..split]);
                tracker.feed(&stream[split..]);
                assert_eq!(tracker.dirty(), expected, "split at {split}");
            }
            let mut bytewise = ModeTracker::new();
            for byte in stream {
                bytewise.feed(core::slice::from_ref(byte));
            }
            assert_eq!(bytewise.dirty(), expected);
        }
    }

    #[test]
    fn several_parameters_in_one_sequence_apply_together() {
        assert!(dirty_after(b"\x1b[?1049;25h"));
        assert!(dirty_after(b"\x1b[?7;2004h"));
        assert!(!dirty_after(b"\x1b[?1049;2004h\x1b[?2004;1049l"));
        assert!(!dirty_after(b"\x1b[?25l\x1b[?1;25h"));
        // An empty parameter is 0 and changes nothing.
        assert!(!dirty_after(b"\x1b[?;25h"));
        assert!(dirty_after(b"\x1b[?;25l"));
    }

    #[test]
    fn unrelated_sequences_are_ignored() {
        let cases: [&[u8]; 10] = [
            b"\x1b[1049h",                 // not private
            b"\x1b[?1049$h",               // intermediate byte
            b"\x1b[?1049m",                // other final byte
            b"\x1b[?1049:1h",              // sub-parameter
            b"\x1b[?47h\x1b[?1047h",       // other alternate-screen modes
            b"\x1b[?12l\x1b[?7l",          // other private modes
            b"\x1b[31;1m\x1b[2J\x1b[H",    // SGR, erase, cursor
            b"\x1b]0;title\x07\x1b7\x1b8", // OSC, save/restore cursor
            b"?1049h ?25l [?2004h",        // the text alone
            b"\x1b[?\xc3\xa91049h",        // non-ASCII byte abandons the control
        ];
        for bytes in cases {
            assert!(!dirty_after(bytes), "{bytes:?}");
        }
    }

    #[test]
    fn oversized_and_hostile_controls_are_bounded_and_ignored() {
        // Exactly MAX_PARAMS parameters are honoured, one more drops the whole control.
        let params = |count: usize| {
            let mut bytes = Vec::from(b"\x1b[?".as_slice());
            for index in 0..count {
                if index > 0 {
                    bytes.push(b';');
                }
                let param: &[u8] = if index == count - 1 { b"1049" } else { b"7" };
                bytes.extend_from_slice(param);
            }
            bytes.push(b'h');
            bytes
        };
        assert!(dirty_after(&params(MAX_PARAMS)));
        assert!(!dirty_after(&params(MAX_PARAMS + 1)));
        // Exactly MAX_CONTROL bytes (ESC to final) are honoured, one more is ignored.
        let sized = |len: usize| {
            let mut bytes = Vec::from(b"\x1b[?".as_slice());
            bytes.resize(len - 5, b'0');
            bytes.extend_from_slice(b"1049h");
            bytes
        };
        assert_eq!(sized(MAX_CONTROL).len(), MAX_CONTROL);
        assert!(dirty_after(&sized(MAX_CONTROL)));
        assert!(!dirty_after(&sized(MAX_CONTROL + 1)));
        // A saturated number is not a tracked mode.
        assert!(!dirty_after(b"\x1b[?99999999999999991049h"));
        // Endless parameters and digits neither panic nor grow anything; ground recovers.
        let mut tracker = ModeTracker::new();
        tracker.feed(b"\x1b[?");
        for _ in 0..64 * 1024 {
            tracker.feed(b"1049;");
        }
        tracker.feed(b"h");
        assert!(!tracker.dirty());
        tracker.feed(b"\x1b[?25l");
        assert!(tracker.dirty());
        // CAN/SUB abort, ESC restarts, other C0 controls execute in place.
        assert!(!dirty_after(b"\x1b[?10\x1849h"));
        assert!(!dirty_after(b"\x1b[?10\x1a49h"));
        assert!(dirty_after(b"\x1b[?10\x1b[?25l"));
        assert!(dirty_after(b"\x1b[?10\n49h"));
        assert!(dirty_after(b"\x1b\x1b\x1b[?2004h"));
    }

    #[test]
    fn relay_streams_are_tracked_separately_and_combined() {
        let mut screen = RelayScreen::new();
        screen.feed(false, b"plain output\n");
        assert!(!screen.needs_reset());
        // Half a switch on each stream is no switch at all.
        screen.feed(false, b"\x1b[?10");
        screen.feed(true, b"49h");
        assert!(!screen.needs_reset());
        screen.feed(true, b"\x1b[?25l");
        assert!(screen.needs_reset());
        screen.feed(true, b"\x1b[?25h");
        assert!(!screen.needs_reset());
        screen.feed(false, TUI_ENTER);
        assert!(screen.needs_reset());
    }
}
