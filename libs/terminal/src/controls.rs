// SPDX-License-Identifier: Apache-2.0 OR MIT
//! VT escape sequence formatting and screen control primitives for `cellos-vt1`.

extern crate alloc;
use alloc::format;
use alloc::vec::Vec;

/// Color representation for `cellos-vt1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

/// SGR (Select Graphic Rendition) text attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextAttributes {
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub reverse: bool,
    pub fg: Option<Color>,
    pub bg: Option<Color>,
}

/// Helper functions to emit `cellos-vt1` escape sequences into a buffer.
pub struct ScreenControls;

impl ScreenControls {
    pub fn move_cursor_to(row: u16, col: u16, out: &mut Vec<u8>) {
        let seq = format!("\x1b[{};{}H", row, col);
        out.extend_from_slice(seq.as_bytes());
    }

    pub fn cursor_up(n: u16, out: &mut Vec<u8>) {
        if n == 0 { return; }
        let seq = format!("\x1b[{}A", n);
        out.extend_from_slice(seq.as_bytes());
    }

    pub fn cursor_down(n: u16, out: &mut Vec<u8>) {
        if n == 0 { return; }
        let seq = format!("\x1b[{}B", n);
        out.extend_from_slice(seq.as_bytes());
    }

    pub fn cursor_forward(n: u16, out: &mut Vec<u8>) {
        if n == 0 { return; }
        let seq = format!("\x1b[{}C", n);
        out.extend_from_slice(seq.as_bytes());
    }

    pub fn cursor_back(n: u16, out: &mut Vec<u8>) {
        if n == 0 { return; }
        let seq = format!("\x1b[{}D", n);
        out.extend_from_slice(seq.as_bytes());
    }

    pub fn save_cursor(out: &mut Vec<u8>) {
        out.extend_from_slice(b"\x1b7");
    }

    pub fn restore_cursor(out: &mut Vec<u8>) {
        out.extend_from_slice(b"\x1b8");
    }

    pub fn clear_screen(out: &mut Vec<u8>) {
        out.extend_from_slice(b"\x1b[2J\x1b[H");
    }

    pub fn clear_line(out: &mut Vec<u8>) {
        out.extend_from_slice(b"\x1b[2K");
    }

    pub fn clear_to_line_end(out: &mut Vec<u8>) {
        out.extend_from_slice(b"\x1b[K");
    }

    pub fn set_cursor_visible(visible: bool, out: &mut Vec<u8>) {
        if visible {
            out.extend_from_slice(b"\x1b[?25h");
        } else {
            out.extend_from_slice(b"\x1b[?25l");
        }
    }

    pub fn enter_alternate_screen(out: &mut Vec<u8>) {
        out.extend_from_slice(b"\x1b[?1049h");
    }

    pub fn leave_alternate_screen(out: &mut Vec<u8>) {
        out.extend_from_slice(b"\x1b[?1049l");
    }

    pub fn enable_bracketed_paste(out: &mut Vec<u8>) {
        out.extend_from_slice(b"\x1b[?2004h");
    }

    pub fn disable_bracketed_paste(out: &mut Vec<u8>) {
        out.extend_from_slice(b"\x1b[?2004l");
    }

    pub fn set_scroll_margins(top: u16, bottom: u16, out: &mut Vec<u8>) {
        let seq = format!("\x1b[{};{}r", top, bottom);
        out.extend_from_slice(seq.as_bytes());
    }

    pub fn reset_attributes(out: &mut Vec<u8>) {
        out.extend_from_slice(b"\x1b[0m");
    }

    pub fn apply_attributes(attrs: &TextAttributes, out: &mut Vec<u8>) {
        let mut codes = Vec::new();
        codes.push("0");
        if attrs.bold { codes.push("1"); }
        if attrs.dim { codes.push("2"); }
        if attrs.italic { codes.push("3"); }
        if attrs.underline { codes.push("4"); }
        if attrs.reverse { codes.push("7"); }

        let sgr_init = format!("\x1b[{}m", codes.join(";"));
        out.extend_from_slice(sgr_init.as_bytes());

        if let Some(fg) = attrs.fg {
            Self::emit_color_code(fg, true, out);
        }
        if let Some(bg) = attrs.bg {
            Self::emit_color_code(bg, false, out);
        }
    }

    fn emit_color_code(color: Color, is_fg: bool, out: &mut Vec<u8>) {
        let base = if is_fg { 30 } else { 40 };
        let bright_base = if is_fg { 90 } else { 100 };

        match color {
            Color::Black => out.extend_from_slice(format!("\x1b[{}m", base).as_bytes()),
            Color::Red => out.extend_from_slice(format!("\x1b[{}m", base + 1).as_bytes()),
            Color::Green => out.extend_from_slice(format!("\x1b[{}m", base + 2).as_bytes()),
            Color::Yellow => out.extend_from_slice(format!("\x1b[{}m", base + 3).as_bytes()),
            Color::Blue => out.extend_from_slice(format!("\x1b[{}m", base + 4).as_bytes()),
            Color::Magenta => out.extend_from_slice(format!("\x1b[{}m", base + 5).as_bytes()),
            Color::Cyan => out.extend_from_slice(format!("\x1b[{}m", base + 6).as_bytes()),
            Color::White => out.extend_from_slice(format!("\x1b[{}m", base + 7).as_bytes()),
            Color::BrightBlack => out.extend_from_slice(format!("\x1b[{}m", bright_base).as_bytes()),
            Color::BrightRed => out.extend_from_slice(format!("\x1b[{}m", bright_base + 1).as_bytes()),
            Color::BrightGreen => out.extend_from_slice(format!("\x1b[{}m", bright_base + 2).as_bytes()),
            Color::BrightYellow => out.extend_from_slice(format!("\x1b[{}m", bright_base + 3).as_bytes()),
            Color::BrightBlue => out.extend_from_slice(format!("\x1b[{}m", bright_base + 4).as_bytes()),
            Color::BrightMagenta => out.extend_from_slice(format!("\x1b[{}m", bright_base + 5).as_bytes()),
            Color::BrightCyan => out.extend_from_slice(format!("\x1b[{}m", bright_base + 6).as_bytes()),
            Color::BrightWhite => out.extend_from_slice(format!("\x1b[{}m", bright_base + 7).as_bytes()),
            Color::Indexed(idx) => {
                let code = if is_fg { 38 } else { 48 };
                out.extend_from_slice(format!("\x1b[{};5;{}m", code, idx).as_bytes());
            }
            Color::Rgb(r, g, b) => {
                let code = if is_fg { 38 } else { 48 };
                out.extend_from_slice(format!("\x1b[{};2;{};{};{}m", code, r, g, b).as_bytes());
            }
        }
    }
}

/// Bounded dimensions of a terminal viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewportSize {
    pub rows: u16,
    pub cols: u16,
    pub size_known: bool,
}

impl Default for ViewportSize {
    fn default() -> Self {
        Self {
            rows: 24,
            cols: 80,
            size_known: false,
        }
    }
}

impl ViewportSize {
    pub const MAX_ROWS: u16 = 256;
    pub const MAX_COLS: u16 = 512;
    pub const MAX_ACTIVE_CELLS: usize = 131072;

    /// Compatibility constructor for trusted static dimensions. Invalid input
    /// is rejected, never silently resized. Fallible frontends use `try_new`.
    pub fn new(rows: u16, cols: u16) -> Self {
        Self::try_new(rows, cols).expect("invalid cellos-vt1 viewport")
    }

    pub fn try_new(rows: u16, cols: u16) -> Result<Self, crate::error::TerminalError> {
        let size = Self { rows, cols, size_known: true };
        size.validate()?;
        Ok(size)
    }

    /// Revalidate public fields at service boundaries before changing state.
    pub fn validate(&self) -> Result<(), crate::error::TerminalError> {
        api::services::session::validate_dimensions(self.rows, self.cols)
            .map_err(|_| crate::error::TerminalError::InvalidInput)
    }

    pub fn cells(&self) -> usize {
        (self.rows as usize) * (self.cols as usize)
    }
}
