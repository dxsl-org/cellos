// SPDX-License-Identifier: MIT
//! `cellos-vt1` byte parser between the session's output streams and the screen.
//!
//! Incremental: state survives any chunk edge. Interpreted (spec §4): UTF-8 text; BEL
//! (no effect), BS, HT, LF, CR; ESC 7/8; CSI A B C D E F G H f J K m s u r S T; DECSET/
//! DECRST ?7 ?25 ?1049 ?2004. Every other sequence is consumed and ignored.
//!
//! Bounds: a control is at most [`MAX_CONTROL`] bytes and [`MAX_PARAMS`] parameters; a
//! longer one is abandoned and the parser returns to ground, so the excess is ordinary
//! text. More parameters make the sequence a no-op. OSC/DCS/SOS/PM/APC strings are
//! discarded until BEL/ST or [`MAX_STRING`] bytes. Nothing here can answer the
//! producer: the parser has no output path, so device queries get no reply, and no OSC
//! has a side effect (title, clipboard, URL, file).

use crate::grid::TerminalGrid;

/// Longest escape or CSI sequence, introducer included.
pub const MAX_CONTROL: usize = 256;
/// Most parameters a CSI may carry and still take effect.
pub const MAX_PARAMS: usize = 16;
/// Longest OSC/DCS (or SOS/PM/APC) string discarded before the parser gives up on it.
pub const MAX_STRING: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Ground,
    /// Partial UTF-8 scalar: `have` of `need` bytes collected.
    Utf8 { need: u8, have: u8 },
    /// After ESC.
    Esc,
    /// ESC followed by intermediate bytes (0x20..=0x2f); the final byte ends it.
    EscIntermediate,
    /// ESC [ ...
    Csi,
    /// OSC, DCS, SOS, PM or APC string: discarded.
    Str,
    /// ESC inside a string: `\` terminates it, anything else abandons it.
    StrEsc,
}

/// Parameters of the CSI being collected.
#[derive(Clone, Copy, Debug)]
struct Csi {
    params: [u16; MAX_PARAMS],
    /// Parameters started so far (may exceed `MAX_PARAMS`, then `ignore` is set).
    count: usize,
    /// `?`, `<`, `=` or `>` as the first byte; 0 otherwise.
    private: u8,
    /// The sequence is outside the profile (intermediates, sub-parameters, misplaced
    /// private marker, too many parameters): it is consumed without effect.
    ignore: bool,
}

impl Csi {
    const EMPTY: Self = Self { params: [0; MAX_PARAMS], count: 0, private: 0, ignore: false };
}

/// Incremental parser; keep one per output stream pair (state survives chunk edges).
pub struct ByteSink {
    state: State,
    utf8: [u8; 4],
    /// Bytes of the current control so far, introducer included.
    len: usize,
    csi: Csi,
}

impl Default for ByteSink {
    fn default() -> Self {
        Self::new()
    }
}

impl ByteSink {
    pub const fn new() -> Self {
        Self { state: State::Ground, utf8: [0; 4], len: 0, csi: Csi::EMPTY }
    }

    /// Drop any half-parsed sequence (new session / restart).
    pub fn reset(&mut self) {
        self.state = State::Ground;
    }

    /// True when no sequence or UTF-8 scalar is pending.
    #[cfg(test)]
    pub fn is_ground(&self) -> bool {
        self.state == State::Ground
    }

    pub fn feed(&mut self, grid: &mut TerminalGrid, bytes: &[u8]) {
        for &byte in bytes {
            self.step(grid, byte);
        }
    }

    fn step(&mut self, grid: &mut TerminalGrid, byte: u8) {
        match self.state {
            State::Ground => self.ground(grid, byte),
            State::Utf8 { need, have } => {
                if (0x80..=0xbf).contains(&byte) {
                    self.utf8[have as usize] = byte;
                    if have + 1 == need {
                        self.state = State::Ground;
                        let ch = core::str::from_utf8(&self.utf8[..need as usize])
                            .ok()
                            .and_then(|s| s.chars().next())
                            .unwrap_or('\u{fffd}');
                        grid.print(ch);
                    } else {
                        self.state = State::Utf8 { need, have: have + 1 };
                    }
                } else {
                    // Truncated scalar: replace it, then handle this byte afresh.
                    self.state = State::Ground;
                    grid.print('\u{fffd}');
                    self.ground(grid, byte);
                }
            }
            State::Esc => self.escape(grid, byte),
            State::EscIntermediate => match byte {
                0x20..=0x2f => self.grow(grid, byte, MAX_CONTROL),
                // Final byte: charset designations and the like are not in the profile.
                0x30..=0x7e => self.state = State::Ground,
                _ => self.interrupt(grid, byte),
            },
            State::Csi => match byte {
                0x20..=0x7e => {
                    if self.len >= MAX_CONTROL {
                        self.abandon(grid, byte);
                        return;
                    }
                    self.len += 1;
                    self.csi_byte(grid, byte);
                }
                _ => self.interrupt(grid, byte),
            },
            State::Str => match byte {
                0x07 | 0x18 | 0x1a => self.state = State::Ground,
                0x1b => self.state = State::StrEsc,
                _ => self.grow(grid, byte, MAX_STRING),
            },
            State::StrEsc => {
                if byte == b'\\' {
                    self.state = State::Ground;
                } else {
                    // ESC not followed by ST starts a new escape.
                    self.begin_escape();
                    self.escape(grid, byte);
                }
            }
        }
    }

    /// One more byte of a sequence that has no effect until its end.
    fn grow(&mut self, grid: &mut TerminalGrid, byte: u8, limit: usize) {
        if self.len >= limit {
            self.abandon(grid, byte);
        } else {
            self.len += 1;
        }
    }

    /// The control exceeded its bound: back to ground without effect; `byte` is ordinary.
    fn abandon(&mut self, grid: &mut TerminalGrid, byte: u8) {
        self.state = State::Ground;
        self.ground(grid, byte);
    }

    /// Bytes that are not part of an escape/CSI body: ESC restarts, CAN/SUB cancel, other
    /// C0 controls execute (the sequence continues inside a CSI, ends otherwise), DEL is
    /// ignored, and 8-bit bytes abandon the sequence and are handled as ordinary output.
    fn interrupt(&mut self, grid: &mut TerminalGrid, byte: u8) {
        match byte {
            0x1b => self.begin_escape(),
            0x18 | 0x1a => self.state = State::Ground,
            0x00..=0x1f => {
                if self.state != State::Csi {
                    self.state = State::Ground;
                }
                control(grid, byte);
            }
            0x7f => {}
            _ => self.abandon(grid, byte),
        }
    }

    fn ground(&mut self, grid: &mut TerminalGrid, byte: u8) {
        match byte {
            0x1b => self.begin_escape(),
            0x00..=0x1f => control(grid, byte),
            0x20..=0x7e => grid.print(char::from(byte)),
            0x7f => {}
            0xc2..=0xdf => self.begin_utf8(2, byte),
            0xe0..=0xef => self.begin_utf8(3, byte),
            0xf0..=0xf4 => self.begin_utf8(4, byte),
            // Stray continuation or invalid lead byte.
            _ => grid.print('\u{fffd}'),
        }
    }

    fn begin_utf8(&mut self, need: u8, lead: u8) {
        self.utf8[0] = lead;
        self.state = State::Utf8 { need, have: 1 };
    }

    fn begin_escape(&mut self) {
        self.state = State::Esc;
        self.len = 1;
    }

    fn escape(&mut self, grid: &mut TerminalGrid, byte: u8) {
        match byte {
            b'[' => {
                self.state = State::Csi;
                self.len = 2;
                self.csi = Csi::EMPTY;
            }
            b']' | b'P' | b'X' | b'^' | b'_' => {
                self.state = State::Str;
                self.len = 2;
            }
            b'7' => {
                self.state = State::Ground;
                grid.save_cursor();
            }
            b'8' => {
                self.state = State::Ground;
                grid.restore_cursor();
            }
            0x20..=0x2f => {
                self.state = State::EscIntermediate;
                self.len = 2;
            }
            // Any other two-byte escape (ESC =, ESC c, ESC M, ...) is not in the profile.
            0x30..=0x7e => self.state = State::Ground,
            _ => self.interrupt(grid, byte),
        }
    }

    fn csi_byte(&mut self, grid: &mut TerminalGrid, byte: u8) {
        let csi = &mut self.csi;
        match byte {
            b'0'..=b'9' => {
                if csi.count == 0 {
                    csi.count = 1;
                }
                if csi.count <= MAX_PARAMS {
                    let param = &mut csi.params[csi.count - 1];
                    *param = param.saturating_mul(10).saturating_add(u16::from(byte - b'0'));
                }
            }
            b';' => {
                // An empty leading parameter counts as one.
                csi.count = csi.count.max(1).saturating_add(1).min(MAX_PARAMS + 1);
                if csi.count > MAX_PARAMS {
                    csi.ignore = true;
                }
            }
            // Sub-parameters are not in the profile.
            b':' => csi.ignore = true,
            b'<'..=b'?' => {
                if self.len == 3 {
                    csi.private = byte;
                } else {
                    csi.ignore = true;
                }
            }
            // No CSI with intermediate bytes is in the profile.
            0x20..=0x2f => csi.ignore = true,
            _ => {
                self.state = State::Ground;
                if !csi.ignore {
                    let csi = self.csi;
                    dispatch(grid, &csi, byte);
                }
            }
        }
    }
}

/// C0 controls with an effect on the screen; BEL and everything else have none.
fn control(grid: &mut TerminalGrid, byte: u8) {
    match byte {
        0x08 => grid.backspace(),
        b'\t' => grid.tab(),
        b'\n' => grid.line_feed(),
        b'\r' => grid.carriage_return(),
        _ => {}
    }
}

fn dispatch(grid: &mut TerminalGrid, csi: &Csi, final_byte: u8) {
    let params = &csi.params[..csi.count.min(MAX_PARAMS)];
    let arg = |i: usize| params.get(i).copied().unwrap_or(0);
    // Count/position parameters: missing or 0 means 1.
    let n = |i: usize| usize::from(arg(i).max(1));
    match (csi.private, final_byte) {
        (0, b'A') => grid.cursor_up(n(0)),
        (0, b'B') => grid.cursor_down(n(0)),
        (0, b'C') => grid.cursor_forward(n(0)),
        (0, b'D') => grid.cursor_back(n(0)),
        (0, b'E') => grid.next_line(n(0)),
        (0, b'F') => grid.previous_line(n(0)),
        (0, b'G') => grid.set_column(n(0)),
        (0, b'H' | b'f') => grid.set_position(n(0), n(1)),
        (0, b'J') => grid.erase_display(arg(0)),
        (0, b'K') => grid.erase_line(arg(0)),
        (0, b'm') => grid.select_graphic_rendition(params),
        // With parameters these are other controls (DECSLRM, ...), not in the profile.
        (0, b's') if params.is_empty() => grid.save_cursor(),
        (0, b'u') if params.is_empty() => grid.restore_cursor(),
        (0, b'r') if params.len() <= 2 => grid.set_scroll_region(usize::from(arg(0)), usize::from(arg(1))),
        (0, b'S') if params.len() <= 1 => grid.scroll_up(n(0)),
        (0, b'T') if params.len() <= 1 => grid.scroll_down(n(0)),
        (b'?', b'h' | b'l') => {
            for &mode in params {
                grid.set_private_mode(mode, final_byte == b'h');
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::grid::{Color, Pen, TerminalCell, Width};
    use std::string::String;
    use std::vec::Vec;

    fn row(grid: &TerminalGrid, r: usize) -> String {
        let text: String = (0..grid.cols)
            .filter(|&c| grid.cell_at(c, r).unwrap().width != Width::Trail)
            .flat_map(|c| {
                let cell = grid.cell_at(c, r).unwrap();
                core::iter::once(cell.ch).chain(cell.mark)
            })
            .collect();
        text.trim_end().into()
    }

    fn screen(grid: &TerminalGrid) -> Vec<String> {
        (0..grid.rows).map(|r| row(grid, r)).collect()
    }

    fn render_sized(cols: usize, rows: usize, chunks: &[&[u8]]) -> (TerminalGrid, ByteSink) {
        let mut grid = TerminalGrid::new(cols, rows);
        let mut sink = ByteSink::new();
        for chunk in chunks {
            sink.feed(&mut grid, chunk);
        }
        (grid, sink)
    }

    fn render(chunks: &[&[u8]]) -> (TerminalGrid, ByteSink) {
        render_sized(40, 6, chunks)
    }

    /// Full observable state: every cell, cursor, modes, pen.
    fn snapshot(grid: &TerminalGrid) -> (Vec<TerminalCell>, (usize, usize), [bool; 4], Pen, (usize, usize)) {
        let cells = (0..grid.rows).flat_map(|r| (0..grid.cols).map(move |c| (c, r))).map(|(c, r)| *grid.cell_at(c, r).unwrap()).collect();
        (
            cells,
            (grid.cursor_x, grid.cursor_y),
            [grid.alternate_active(), grid.cursor_visible(), grid.bracketed_paste(), grid.autowrap()],
            grid.pen(),
            grid.margins(),
        )
    }

    /// Feeding `bytes` in two chunks at every split point gives the same state as one chunk.
    fn assert_split_invariant(cols: usize, rows: usize, bytes: &[u8]) {
        let (whole, _) = render_sized(cols, rows, &[bytes]);
        let expected = snapshot(&whole);
        for split in 0..=bytes.len() {
            let (grid, sink) = render_sized(cols, rows, &[&bytes[..split], &bytes[split..]]);
            assert_eq!(snapshot(&grid), expected, "split at {split}");
            assert!(sink.is_ground());
        }
        // And one byte at a time.
        let chunks: Vec<&[u8]> = bytes.chunks(1).collect();
        let (grid, _) = render_sized(cols, rows, &chunks);
        assert_eq!(snapshot(&grid), expected, "byte-at-a-time");
    }

    #[test]
    fn plain_text_cr_lf_backspace_and_tab() {
        let (grid, _) = render(&[b"ab\r\ncd\x08X\tY"]);
        assert_eq!(row(&grid, 0), "ab");
        assert_eq!(row(&grid, 1), "cX      Y");
        assert_eq!(grid.cursor_y, 1);
        let (grid, _) = render(&[b"hello\rJ"]);
        assert_eq!(row(&grid, 0), "Jello");
        // LF starts the next line at column 0 (no ONLCR in CellOS sessions).
        let (grid, _) = render(&[b"ab\ncd"]);
        assert_eq!(screen(&grid)[..2], ["ab", "cd"]);
    }

    #[test]
    fn the_shell_erase_idiom_removes_the_character() {
        let (grid, _) = render(&[b"abc\x08 \x08"]);
        assert_eq!(row(&grid, 0), "ab");
        assert_eq!(grid.cursor_x, 2);
        // BS itself is not destructive.
        let (grid, _) = render(&[b"abc\x08\x08"]);
        assert_eq!(row(&grid, 0), "abc");
        assert_eq!(grid.cursor_x, 1);
    }

    #[test]
    fn other_controls_and_del_are_ignored() {
        let (grid, _) = render(&[b"a\x07\x00\x01\x7fb\x0b\x0c\x0e\x0f"]);
        assert_eq!(row(&grid, 0), "ab");
        assert_eq!(grid.cursor_y, 0);
    }

    #[test]
    fn tab_stops_every_eight_columns_and_stop_at_the_last_column() {
        let (grid, _) = render_sized(20, 2, &[b"\tA\tB\tC\tD"]);
        assert_eq!(row(&grid, 0), "        A       B  D");
        assert_eq!(grid.cursor_x, 19);
    }

    #[test]
    fn cursor_addressing_moves_and_clamps_at_the_edges() {
        let (grid, _) = render_sized(10, 4, &[b"\x1b[2;3HA\x1b[HB\x1b[4;10HC\x1b[99;99HD"]);
        assert_eq!(screen(&grid), ["B", "  A", "", "         D"]);
        assert_eq!((grid.cursor_x, grid.cursor_y), (9, 3));
        // 0 and missing parameters mean 1; `f` is the same as `H`.
        let (grid, _) = render_sized(10, 4, &[b"xx\x1b[0;0fA\x1b[;5fB"]);
        assert_eq!(row(&grid, 0), "Ax  B");
        // Relative moves clamp at the screen edges.
        let (grid, _) = render_sized(10, 4, &[b"\x1b[2;5H\x1b[9AA\x1b[99DB\x1b[99CC\x1b[99BD"]);
        assert_eq!(screen(&grid), ["B   A    C", "", "", "         D"]);
        let (grid, _) = render_sized(10, 4, &[b"\x1b[3;5H\x1b[2EA\x1b[3FB\x1b[7GC"]);
        assert_eq!(screen(&grid), ["B     C", "", "", "A"]);
    }

    #[test]
    fn erase_display_and_line_modes() {
        let fill = b"0123456789\x1b[2;1Habcdefghij\x1b[3;1HABCDEFGHIJ\x1b[2;5H";
        let erase = |seq: &[u8]| {
            let (grid, _) = render_sized(10, 3, &[fill, seq]);
            screen(&grid)
        };
        assert_eq!(erase(b"\x1b[K"), ["0123456789", "abcd", "ABCDEFGHIJ"]);
        assert_eq!(erase(b"\x1b[0K"), ["0123456789", "abcd", "ABCDEFGHIJ"]);
        assert_eq!(erase(b"\x1b[1K"), ["0123456789", "     fghij", "ABCDEFGHIJ"]);
        assert_eq!(erase(b"\x1b[2K"), ["0123456789", "", "ABCDEFGHIJ"]);
        assert_eq!(erase(b"\x1b[J"), ["0123456789", "abcd", ""]);
        assert_eq!(erase(b"\x1b[1J"), ["", "     fghij", "ABCDEFGHIJ"]);
        assert_eq!(erase(b"\x1b[2J"), ["", "", ""]);
        // Unsupported modes change nothing; erase never moves the cursor.
        assert_eq!(erase(b"\x1b[3J\x1b[5K"), ["0123456789", "abcdefghij", "ABCDEFGHIJ"]);
        let (grid, _) = render_sized(10, 3, &[fill, b"\x1b[2J"]);
        assert_eq!((grid.cursor_x, grid.cursor_y), (4, 1));
    }

    #[test]
    fn erase_uses_the_current_background_and_default_attributes() {
        let (grid, _) = render_sized(10, 2, &[b"\x1b[1;7;31;44mab\x1b[K"]);
        let erased = grid.cell_at(5, 0).unwrap();
        assert_eq!(erased.pen, Pen { bg: Color::Indexed(4), ..Pen::DEFAULT });
        assert_eq!(erased.ch, ' ');
    }

    #[test]
    fn sgr_colors_and_attributes() {
        let (grid, _) = render(&[b"\x1b[1;31mA\x1b[22;7;42mB\x1b[27;39;49mC\x1b[95;104mD\x1b[38;5;200;48;2;1;2;3mE\x1b[0mF\x1b[1mG\x1b[mH"]);
        let pen = |c: usize| grid.cell_at(c, 0).unwrap().pen;
        assert_eq!(pen(0), Pen { fg: Color::Indexed(1), bold: true, ..Pen::DEFAULT });
        assert_eq!(pen(1), Pen { fg: Color::Indexed(1), bg: Color::Indexed(2), inverse: true, bold: false });
        assert_eq!(pen(2), Pen::DEFAULT);
        assert_eq!(pen(3), Pen { fg: Color::Indexed(13), bg: Color::Indexed(12), ..Pen::DEFAULT });
        assert_eq!(pen(4), Pen { fg: Color::Indexed(200), bg: Color::Rgb(1, 2, 3), ..Pen::DEFAULT });
        assert_eq!(pen(5), Pen::DEFAULT);
        assert_eq!(pen(6), Pen { bold: true, ..Pen::DEFAULT });
        assert_eq!(pen(7), Pen::DEFAULT);
        assert_eq!(row(&grid, 0), "ABCDEFGH");
    }

    #[test]
    fn sgr_accepts_unsupported_attributes_and_stops_at_a_malformed_extended_color() {
        let (grid, _) = render(&[b"\x1b[2;3;4;5;9;31mA\x1b[0m\x1b[38;5;300;1mB\x1b[0m\x1b[32;38;2;1;2mC\x1b[0m\x1b[38;7;1mD"]);
        let pen = |c: usize| grid.cell_at(c, 0).unwrap().pen;
        assert_eq!(pen(0), Pen { fg: Color::Indexed(1), ..Pen::DEFAULT });
        // Index 300 is malformed: the rest of that SGR (bold) is not applied.
        assert_eq!(pen(1), Pen::DEFAULT);
        assert_eq!(pen(2), Pen { fg: Color::Indexed(2), ..Pen::DEFAULT });
        assert_eq!(pen(3), Pen::DEFAULT);
    }

    #[test]
    fn save_and_restore_cursor_with_esc_and_csi() {
        let (grid, _) = render_sized(10, 3, &[b"\x1b[2;3H\x1b[31m\x1b7\x1b[HX\x1b[0m\x1b8Y"]);
        assert_eq!(screen(&grid), ["X", "  Y", ""]);
        assert_eq!(grid.cell_at(2, 1).unwrap().pen.fg, Color::Indexed(1));
        let (grid, _) = render_sized(10, 3, &[b"\x1b[3;4H\x1b[s\x1b[HX\x1b[uY"]);
        assert_eq!(screen(&grid), ["X", "", "   Y"]);
        // Restore without a save homes the cursor.
        let (grid, _) = render_sized(10, 3, &[b"\x1b[3;4H\x1b8Z"]);
        assert_eq!(row(&grid, 0), "Z");
    }

    #[test]
    fn autowrap_wraps_once_and_can_be_disabled() {
        let (grid, _) = render_sized(5, 3, &[b"abcde"]);
        // Pending wrap: the cursor stays on the last column until the next printable.
        assert_eq!((grid.cursor_x, grid.cursor_y), (4, 0));
        let (grid, _) = render_sized(5, 3, &[b"abcdefg"]);
        assert_eq!(screen(&grid), ["abcde", "fg", ""]);
        // CR after the last column does not produce an empty line.
        let (grid, _) = render_sized(5, 3, &[b"abcde\r\nX"]);
        assert_eq!(screen(&grid), ["abcde", "X", ""]);
        let (grid, _) = render_sized(5, 3, &[b"\x1b[?7labcdefg\x1b[?7hXY"]);
        assert_eq!(screen(&grid), ["abcdX", "Y", ""]);
    }

    #[test]
    fn scroll_margins_with_lf_su_and_sd() {
        let base = b"1\n2\n3\n4\n5";
        // LF at the bottom margin scrolls only the region; lines outside stay.
        let (grid, _) = render_sized(5, 5, &[base, b"\x1b[2;4r\x1b[4;1H\nX"]);
        assert_eq!(screen(&grid), ["1", "3", "4", "X", "5"]);
        assert_eq!(grid.scrollback_len(), 0);
        // DECSTBM homes the cursor.
        let (grid, _) = render_sized(5, 5, &[base, b"\x1b[2;4rH"]);
        assert_eq!(row(&grid, 0), "H");
        // SU / SD move the region only.
        let (grid, _) = render_sized(5, 5, &[base, b"\x1b[2;4r\x1b[2S"]);
        assert_eq!(screen(&grid), ["1", "4", "", "", "5"]);
        let (grid, _) = render_sized(5, 5, &[base, b"\x1b[2;4r\x1bT"]);
        assert_eq!(screen(&grid), ["1", "2", "3", "4", "5"]);
        let (grid, _) = render_sized(5, 5, &[base, b"\x1b[2;4r\x1b[T"]);
        assert_eq!(screen(&grid), ["1", "", "2", "3", "5"]);
        let (grid, _) = render_sized(5, 5, &[base, b"\x1b[2;4r\x1b[9T"]);
        assert_eq!(screen(&grid), ["1", "", "", "", "5"]);
        // Below the region, LF at the last row does not scroll anything.
        let (grid, _) = render_sized(5, 5, &[base, b"\x1b[2;3r\x1b[5;1H\nZ"]);
        assert_eq!(screen(&grid), ["1", "2", "3", "4", "Z"]);
        // Invalid regions are ignored; CUU/CUD stop at the margins.
        let (grid, _) = render_sized(5, 5, &[base, b"\x1b[4;2r\x1b[3;3r"]);
        assert_eq!(grid.margins(), (0, 4));
        let (grid, _) = render_sized(5, 5, &[b"\x1b[2;4r\x1b[3;1H\x1b[9AU\x1b[9BD"]);
        assert_eq!(screen(&grid), ["", "U", "", " D", ""]);
        // Full-screen scrolling feeds scrollback; `r` without parameters resets.
        let (grid, _) = render_sized(5, 5, &[base, b"\x1b[2;4r\x1b[r\x1b[5;1H\n"]);
        assert_eq!(grid.scrollback_len(), 1);
    }

    #[test]
    fn alternate_screen_enter_and_exit_restores_the_primary_screen_and_cursor() {
        let prompt = b"line one\r\nCellos > tui";
        let (before, _) = render_sized(20, 4, &[prompt]);
        let (mut grid, mut sink) = render_sized(20, 4, &[prompt, b"\x1b[?1049h"]);
        assert!(grid.alternate_active());
        assert_eq!(screen(&grid), ["", "", "", ""]);
        sink.feed(&mut grid, b"\x1b[?25l\x1b[H\x1b[44mTUI\x1b[2;1Hframe\x1b[0m\x1b7\x1b[4;1Hbottom");
        assert_eq!(screen(&grid), ["TUI", "frame", "", "bottom"]);
        sink.feed(&mut grid, b"\x1b[?1049l\x1b[?25h");
        assert!(!grid.alternate_active());
        assert_eq!(snapshot(&grid), snapshot(&before));
        // Leaving when not in the alternate screen is a no-op (soft reset after a plain
        // program leaves everything as it was).
        sink.feed(&mut grid, b"\x1b[?1049l\x1b[?25h\x1b[?2004l\x1b[0m");
        assert_eq!(snapshot(&grid), snapshot(&before));
        // Entering twice keeps the first saved primary screen.
        sink.feed(&mut grid, b"\x1b[?1049hA\x1b[?1049hB\x1b[?1049l");
        assert_eq!(snapshot(&grid), snapshot(&before));
    }

    #[test]
    fn private_modes_cursor_visibility_and_bracketed_paste() {
        let (grid, _) = render(&[b"\x1b[?25l\x1b[?2004h"]);
        assert!(!grid.cursor_visible() && grid.bracketed_paste());
        let (grid, _) = render(&[b"\x1b[?25;2004l"]);
        assert!(!grid.cursor_visible() && !grid.bracketed_paste());
        let (grid, _) = render(&[b"\x1b[?25l\x1b[?25;2004;1049h"]);
        assert!(grid.cursor_visible() && grid.bracketed_paste() && grid.alternate_active());
        // Without the `?` these are other (unsupported) modes.
        let (grid, _) = render(&[b"\x1b[25l\x1b[2004h"]);
        assert!(grid.cursor_visible() && !grid.bracketed_paste());
    }

    #[test]
    fn wide_characters_take_two_cells_and_the_cursor_never_rests_on_a_trailing_half() {
        let (grid, _) = render_sized(10, 2, &[b"a\xe4\xb8\x96b"]);
        assert_eq!(grid.cell_at(1, 0).unwrap().width, Width::Wide);
        assert_eq!(grid.cell_at(2, 0).unwrap().width, Width::Trail);
        assert_eq!(row(&grid, 0), "a世b");
        assert_eq!(grid.cursor_x, 4);
        // Addressing the trailing half lands on the lead cell.
        let (grid, _) = render_sized(10, 2, &[b"a\xe4\xb8\x96\x1b[1;3H"]);
        assert_eq!(grid.cursor_x, 1);
        let (grid, _) = render_sized(10, 2, &[b"a\xe4\xb8\x96b\x1b[2D"]);
        assert_eq!(grid.cursor_x, 1);
        // Overwriting either half removes the whole character.
        let (grid, _) = render_sized(10, 2, &[b"a\xe4\xb8\x96b\x1b[1;2HX"]);
        assert_eq!(row(&grid, 0), "aX b");
        // A wide character straddling the lead of another removes that one.
        let (grid, _) = render_sized(10, 2, &[b"ab\xe4\xb8\x96\x1b[1;2H\xe4\xb8\x96"]);
        assert_eq!(row(&grid, 0), "a世");
        assert_eq!(grid.cell_at(1, 0).unwrap().width, Width::Wide);
        assert_eq!(grid.cell_at(3, 0).unwrap().width, Width::Narrow);
        // Erasing half of a wide character removes all of it.
        let (grid, _) = render_sized(10, 2, &[b"ab\xe4\xb8\x96cd\x1b[1;3H\x1b[1K"]);
        assert_eq!(row(&grid, 0), "    cd");
        assert_eq!(grid.cell_at(3, 0).unwrap().width, Width::Narrow);
    }

    #[test]
    fn a_wide_character_at_the_last_column_wraps_whole() {
        let (grid, _) = render_sized(5, 3, &[b"abcd\xe4\xb8\x96"]);
        assert_eq!(screen(&grid), ["abcd", "世", ""]);
        assert_eq!((grid.cursor_x, grid.cursor_y), (2, 1));
        // Exactly filling the line: pending wrap, cursor on the lead cell.
        let (grid, _) = render_sized(5, 3, &[b"abc\xe4\xb8\x96"]);
        assert_eq!(row(&grid, 0), "abc世");
        assert_eq!((grid.cursor_x, grid.cursor_y), (3, 0));
        let (grid, _) = render_sized(5, 3, &[b"abc\xe4\xb8\x96Z"]);
        assert_eq!(screen(&grid), ["abc世", "Z", ""]);
        // Without autowrap the wide character replaces the last two columns.
        let (grid, _) = render_sized(5, 3, &[b"\x1b[?7labcd\xe4\xb8\x96"]);
        assert_eq!(screen(&grid), ["abc世", "", ""]);
        // A one-column screen cannot show a wide character at all.
        let (grid, _) = render_sized(1, 2, &[b"\xe4\xb8\x96x"]);
        assert_eq!(screen(&grid), ["x", ""]);
    }

    #[test]
    fn combining_marks_join_the_previous_cell() {
        let (grid, _) = render_sized(10, 2, &[b"e\xcc\x81x"]);
        assert_eq!(grid.cell_at(0, 0).unwrap().mark, Some('\u{301}'));
        assert_eq!(row(&grid, 0), "e\u{301}x");
        assert_eq!(grid.cursor_x, 2);
        // Onto a wide character, at the last column (pending wrap), and at line start.
        let (grid, _) = render_sized(10, 2, &[b"\xe4\xb8\x96\xcc\x81"]);
        assert_eq!(grid.cell_at(0, 0).unwrap().mark, Some('\u{301}'));
        let (grid, _) = render_sized(3, 2, &[b"abc\xcc\x81"]);
        assert_eq!(grid.cell_at(2, 0).unwrap().mark, Some('\u{301}'));
        let (grid, _) = render_sized(3, 2, &[b"\xcc\x81a"]);
        assert_eq!(row(&grid, 0), "a");
        // Only one mark is stored per cell.
        let (grid, _) = render_sized(10, 2, &[b"a\xcc\x81\xcc\x82"]);
        assert_eq!(row(&grid, 0), "a\u{301}");
    }

    #[test]
    fn fragmented_controls_and_utf8_give_the_same_screen_at_every_split() {
        assert_split_invariant(
            20,
            5,
            b"\x1b[2;3H\x1b[1;38;5;196;48;2;10;20;30mX\x1b[0m\xe4\xb8\x96e\xcc\x81\x1b7\x1b[?1049h\x1b[?25lalt\x1b[?1049l\x1b[?25h\x1b8\x1b[3;4r\x1b[2S\x1b]0;title\x07\x1bP1$qm\x1b\\\x1b[K\x1b[1J!",
        );
        assert_split_invariant(10, 3, "aé€🙂z\r\n\x1b[31mred\x1b[m".as_bytes());
    }

    #[test]
    fn utf8_scalars_survive_chunk_boundaries() {
        let bytes = "aé€🙂z".as_bytes();
        for split in 0..=bytes.len() {
            let (grid, _) = render(&[&bytes[..split], &bytes[split..]]);
            assert_eq!(row(&grid, 0), "aé€🙂z", "split {split}");
        }
    }

    #[test]
    fn invalid_utf8_becomes_replacement_without_losing_the_next_byte() {
        // Truncated lead then ASCII, overlong lead, lone continuation, surrogate encoding.
        let (grid, _) = render(&[b"\xe2\x82A"]);
        assert_eq!(row(&grid, 0), "\u{fffd}A");
        let (grid, _) = render(&[b"\xc0\x80B"]);
        assert_eq!(row(&grid, 0), "\u{fffd}\u{fffd}B");
        let (grid, _) = render(&[b"\x80C"]);
        assert_eq!(row(&grid, 0), "\u{fffd}C");
        let (grid, _) = render(&[b"\xed\xa0\x80D"]);
        assert_eq!(row(&grid, 0), "\u{fffd}D");
        // A truncated scalar before ESC still lets the sequence run.
        let (grid, _) = render(&[b"\xe2\x1b[31mE"]);
        assert_eq!(row(&grid, 0), "\u{fffd}E");
        assert_eq!(grid.cell_at(1, 0).unwrap().pen.fg, Color::Indexed(1));
    }

    #[test]
    fn controls_inside_csi_execute_and_cancel_or_invalid_bytes_end_it() {
        let (grid, _) = render(&[b"\x1b[1\n2mZ"]);
        assert_eq!(row(&grid, 0), "");
        assert_eq!(row(&grid, 1), "Z");
        assert_eq!(grid.cell_at(0, 1).unwrap().pen.fg, Color::Default);
        // CAN/SUB cancel the sequence; a high byte abandons it and is then ordinary output.
        let (grid, _) = render(&[b"\x1b[31\x18Q"]);
        assert_eq!(row(&grid, 0), "Q");
        assert_eq!(grid.cell_at(0, 0).unwrap().pen, Pen::DEFAULT);
        let (grid, _) = render(&[b"\x1b[1\x1aQ"]);
        assert_eq!(row(&grid, 0), "Q");
        let (grid, _) = render(&[b"\x1b[1\xc3\xa9"]);
        assert_eq!(row(&grid, 0), "é");
        // NUL and DEL inside a CSI are ignored and the sequence still completes.
        let (grid, _) = render(&[b"\x1b[3\x001\x7fmR"]);
        assert_eq!(grid.cell_at(0, 0).unwrap().pen.fg, Color::Indexed(1));
        // ESC restarts: the first CSI is dropped, the second runs.
        let (grid, _) = render(&[b"\x1b[31\x1b[32mG"]);
        assert_eq!(grid.cell_at(0, 0).unwrap().pen.fg, Color::Indexed(2));
    }

    #[test]
    fn hostile_seventeen_parameters_make_the_sequence_a_no_op() {
        let mut seq = Vec::from(&b"\x1b["[..]);
        for _ in 0..16 {
            seq.extend_from_slice(b"1;");
        }
        seq.extend_from_slice(b"31mA");
        let (grid, sink) = render(&[&seq]);
        assert_eq!(row(&grid, 0), "A");
        assert_eq!(grid.cell_at(0, 0).unwrap().pen, Pen::DEFAULT);
        assert!(sink.is_ground());
        // Exactly sixteen still apply; huge values saturate instead of overflowing.
        let (grid, _) = render(&[b"\x1b[1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;31mB\x1b[99999999999;99999999999HC"]);
        assert_eq!(grid.cell_at(0, 0).unwrap().pen.fg, Color::Indexed(1));
        assert_eq!((grid.cursor_x, grid.cursor_y), (39, 5));
        // Thousands of separators never grow anything and recover to ground.
        let mut seq = Vec::from(&b"\x1b["[..]);
        seq.extend(core::iter::repeat(b';').take(200));
        seq.extend_from_slice(b"mD");
        let (grid, sink) = render(&[&seq]);
        assert_eq!(row(&grid, 0), "D");
        assert!(sink.is_ground());
    }

    #[test]
    fn hostile_300_byte_csi_is_abandoned_at_256_bytes() {
        let mut input = Vec::new();
        input.extend_from_slice(b"\x1b[");
        input.extend(core::iter::repeat(b'1').take(298));
        input.extend_from_slice(b"mAFTER");
        let (grid, sink) = render_sized(80, 3, &[&input]);
        // ESC [ plus 254 digits fill the 256-byte bound; the rest is ordinary text.
        let expected: String = core::iter::repeat('1').take(298 - (MAX_CONTROL - 2)).chain("mAFTER".chars()).collect();
        assert_eq!(row(&grid, 0), expected);
        assert!(sink.is_ground());
        assert_eq!(grid.pen(), Pen::DEFAULT);
        // A 256-byte sequence is still in bounds and takes effect.
        let mut input = Vec::from(&b"\x1b["[..]);
        input.extend(core::iter::repeat(b'0').take(MAX_CONTROL - 5));
        input.extend_from_slice(b"31mX");
        let (grid, _) = render(&[&input]);
        assert_eq!(grid.cell_at(0, 0).unwrap().pen.fg, Color::Indexed(1));
        // Escape intermediates are bounded the same way.
        let mut input = Vec::from(&b"\x1b"[..]);
        input.extend(core::iter::repeat(b' ').take(300));
        input.extend_from_slice(b"Y");
        let (grid, sink) = render_sized(80, 3, &[&input]);
        assert!(sink.is_ground());
        assert!(row(&grid, 0).ends_with('Y'));
    }

    #[test]
    fn hostile_5_kib_osc_is_discarded_for_4_kib_then_ground_recovers() {
        let mut input = Vec::from(&b"\x1b]0;"[..]);
        input.extend(core::iter::repeat(b'x').take(5 * 1024));
        input.extend_from_slice(b"\r\nTAIL");
        let (grid, sink) = render_sized(512, 8, &[&input]);
        assert!(sink.is_ground());
        // Everything past the 4 KiB bound is ordinary text (wrapping over 512 columns).
        let shown = 4 + 5 * 1024 - MAX_STRING;
        let xs: String = (0..3).map(|r| row(&grid, r)).collect();
        assert_eq!(xs, core::iter::repeat('x').take(shown).collect::<String>());
        assert_eq!(row(&grid, 3), "TAIL");
        // Terminated strings of every kind have no effect, also when split.
        let strings: &[&[u8]] = &[
            b"\x1b]0;title\x07A",
            b"\x1b]52;c;aGVsbG8=\x07B",
            b"\x1b]8;;file:///etc/passwd\x1b\\C",
            b"\x1bP1$qm\x1b\\D",
            b"\x1b_apc\x1b\\E",
            b"\x1b^pm\x07F",
            b"\x1bXsos\x18G",
        ];
        for seq in strings {
            let (grid, _) = render(&[seq]);
            assert_eq!(row(&grid, 0).len(), 1, "{seq:?}");
            assert_split_invariant(10, 2, seq);
        }
        // ESC inside a string not followed by `\` starts a new escape.
        let (grid, _) = render(&[b"\x1b]0;t\x1b[31mH"]);
        assert_eq!(row(&grid, 0), "H");
        assert_eq!(grid.cell_at(0, 0).unwrap().pen.fg, Color::Indexed(1));
    }

    #[test]
    fn device_queries_and_unsupported_sequences_change_nothing_and_get_no_reply() {
        // The parser has no output channel at all: `feed` only borrows the screen. Every
        // query here is consumed in bounds and leaves the full state untouched.
        let (before, _) = render(&[b"ok"]);
        let queries: &[&[u8]] = &[
            b"\x1b[c", b"\x1b[>c", b"\x1b[=c", b"\x1b[5n", b"\x1b[6n", b"\x1b[?6n", b"\x1b[18t", b"\x1b[14t",
            b"\x1b]10;?\x07", b"\x1b]11;?\x1b\\", b"\x1bP+q544e\x1b\\", b"\x1bZ", b"\x1b[?1000h", b"\x1b[!p",
            b"\x1b[ q", b"\x1b[4h", b"\x1b[?1;2c", b"\x1bc", b"\x1b(0", b"\x1b#8", b"\x1b[1:2m", b"\x1b[>4;1m",
            b"\x1b[5;5s", b"\x1b[1;2;3r", b"\x1b[1;2;3;4;5T", b"\x1bM", b"\x1bD", b"\x1bE",
        ];
        for query in queries {
            let (grid, sink) = render(&[b"ok", query]);
            assert!(sink.is_ground(), "{query:?}");
            assert_eq!(snapshot(&grid), snapshot(&before), "{query:?}");
        }
    }

    #[test]
    fn two_byte_and_intermediate_escapes_are_consumed() {
        let (grid, _) = render(&[b"a\x1b7b\x1b=c\x1b(0d\x1b#8e"]);
        assert_eq!(row(&grid, 0), "abcde");
        // ESC inside ESC restarts it; a control after ESC still executes.
        let (grid, _) = render(&[b"\x1b\x1b[mA\x1b\nB"]);
        assert_eq!(row(&grid, 0), "A");
        assert_eq!(row(&grid, 1), "B");
    }

    #[test]
    fn reset_drops_a_half_parsed_sequence() {
        let mut grid = TerminalGrid::new(20, 3);
        let mut sink = ByteSink::new();
        sink.feed(&mut grid, b"\x1b[3");
        sink.reset();
        sink.feed(&mut grid, b"1mX");
        assert_eq!(row(&grid, 0), "1mX");
    }

    #[test]
    fn long_output_scrolls_into_the_grid_without_panicking() {
        let mut grid = TerminalGrid::new(10, 3);
        let mut sink = ByteSink::new();
        for i in 0..50u8 {
            sink.feed(&mut grid, &[b'a' + (i % 26), b'\r', b'\n']);
        }
        assert_eq!(grid.cursor_y, 2);
        assert!(grid.scrollback_len() >= 47);
    }

    #[test]
    fn random_bytes_never_panic_and_keep_every_bound() {
        // Deterministic xorshift stream over all byte values, biased toward ESC/CSI bytes.
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for (cols, rows) in [(1, 1), (2, 1), (7, 3), (80, 24)] {
            let mut grid = TerminalGrid::new(cols, rows);
            let mut sink = ByteSink::new();
            for _ in 0..20_000 {
                let r = next();
                let byte = match r % 8 {
                    0 => 0x1b,
                    1 => b"[;?0123456789"[(r >> 8) as usize % 13],
                    2 => b"ABCDEFGHJKSTfhlmrsu78"[(r >> 8) as usize % 21],
                    _ => (r >> 16) as u8,
                };
                sink.feed(&mut grid, &[byte]);
                assert!(grid.cursor_x < grid.cols && grid.cursor_y < grid.rows);
                assert_ne!(grid.cell_at(grid.cursor_x, grid.cursor_y).unwrap().width, Width::Trail);
            }
            assert!(grid.scrollback_len() <= crate::grid::MAX_SCROLLBACK_LINES);
        }
    }
}
