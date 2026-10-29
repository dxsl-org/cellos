// SPDX-License-Identifier: Apache-2.0 OR MIT
//! `ratatui_core::backend::Backend` over a `cellos-vt1` byte sink.
//!
//! Output profile: CUP, ED/EL, SGR (reset, bold/dim/italic/underline/reversed,
//! 16/256/RGB colors) and, through the SDK's [`TerminalMode`] transitions only,
//! cursor visibility (DECTCEM), alternate screen (1049) and bracketed paste
//! (2004). The backend never sends a query sequence: size comes from the
//! session ([`CellosBackend::set_size`]) and the cursor position is tracked
//! locally.

use alloc::vec::Vec;
use core::fmt;
use ratatui_core::backend::{Backend, ClearType, WindowSize};
use ratatui_core::buffer::Cell;
use ratatui_core::layout::{Position, Size};
use ratatui_core::style::{Color, Modifier};
use ratatui_core::text::Span;
use terminal_sdk::controls::ScreenControls;
use terminal_sdk::mode::{ModeGuard, TerminalMode};
use terminal_sdk::stream::Endpoint;
use terminal_sdk::ViewportSize;

/// Destination of rendered bytes.
pub trait ByteSink {
    type Error: fmt::Debug;
    /// Largest slice handed to one [`ByteSink::write`] call.
    const MAX_WRITE: usize;
    /// Writes a prefix of `bytes` (at most [`ByteSink::MAX_WRITE`]) and returns
    /// its length. Zero or more than offered is reported as [`Error::WriteCount`].
    fn write(&mut self, bytes: &[u8]) -> Result<usize, Self::Error>;
}

/// A session's stdout endpoint (`TerminalSession::stdout`). Writes block until
/// the stream accepts bytes, like a terminal write; one stream frame is 512 bytes.
impl ByteSink for Endpoint {
    type Error = terminal_sdk::Status;
    const MAX_WRITE: usize = 512;
    fn write(&mut self, bytes: &[u8]) -> Result<usize, Self::Error> {
        Endpoint::write(*self, bytes, u64::MAX)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error<E> {
    /// The sink failed; buffered output was discarded and the tracked cursor and
    /// style are unknown until re-established (the next frame re-emits them).
    Write(E),
    /// The sink reported accepting zero bytes or more than it was offered.
    WriteCount(usize),
    /// Dimensions outside the `cellos-vt1` viewport bounds.
    InvalidSize { cols: u16, rows: u16 },
    /// The cursor position is not tracked and is never queried from the terminal.
    CursorUnknown,
    /// The operation needs a sequence outside the output profile (scrolling).
    Unsupported,
}

impl<E: fmt::Debug> fmt::Display for Error<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Write(error) => write!(f, "terminal write failed: {error:?}"),
            Self::WriteCount(count) => {
                write!(f, "terminal write reported an invalid count {count}")
            }
            Self::InvalidSize { cols, rows } => write!(f, "invalid terminal size {cols}x{rows}"),
            Self::CursorUnknown => f.write_str("terminal cursor position is unknown"),
            Self::Unsupported => f.write_str("operation outside the cellos-vt1 output profile"),
        }
    }
}

impl<E: fmt::Debug> core::error::Error for Error<E> {}

/// Screen state while a TUI runs: alternate screen, hidden cursor, bracketed paste.
pub const TUI_SCREEN: TerminalMode = TerminalMode {
    cursor_visible: false,
    ..TerminalMode::RAW_TUI
};

const PROFILE_MODIFIERS: Modifier = Modifier::BOLD
    .union(Modifier::DIM)
    .union(Modifier::ITALIC)
    .union(Modifier::UNDERLINED)
    .union(Modifier::REVERSED);

/// Graphic rendition the terminal is known to have.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Pen {
    fg: Color,
    bg: Color,
    modifier: Modifier,
}

impl Pen {
    const DEFAULT: Self = Self {
        fg: Color::Reset,
        bg: Color::Reset,
        modifier: Modifier::empty(),
    };
    fn of(cell: &Cell) -> Self {
        Self {
            fg: cell.fg,
            bg: cell.bg,
            modifier: cell.modifier.intersection(PROFILE_MODIFIERS),
        }
    }
}

/// Ratatui backend writing `cellos-vt1` bytes to `W`.
///
/// Output is buffered and handed to the sink in [`ByteSink::MAX_WRITE`] chunks
/// as soon as a chunk is full and on [`Backend::flush`] (which
/// `Terminal::draw` calls once per frame). Dropping the backend flushes and
/// restores the screen mode baseline ([`TerminalMode::CANONICAL`]), best effort.
pub struct CellosBackend<W: ByteSink> {
    sink: W,
    out: Vec<u8>,
    size: Size,
    cursor: Option<Position>,
    pen: Option<Pen>,
    screen: ModeGuard,
    /// Screen mode as of the last fully written buffer.
    flushed: TerminalMode,
}

impl<W: ByteSink> CellosBackend<W> {
    /// The terminal is assumed to start in [`TerminalMode::CANONICAL`] with
    /// unknown cursor position and rendition.
    pub fn new(sink: W, cols: u16, rows: u16) -> Result<Self, Error<W::Error>> {
        Ok(Self {
            sink,
            out: Vec::new(),
            size: checked_size(cols, rows)?,
            cursor: None,
            pen: None,
            screen: ModeGuard::new(TerminalMode::CANONICAL),
            flushed: TerminalMode::CANONICAL,
        })
    }

    pub fn sink(&self) -> &W {
        &self.sink
    }

    pub fn sink_mut(&mut self) -> &mut W {
        &mut self.sink
    }

    /// Applies a session Resize. `Terminal::draw` notices the change, clears the
    /// screen and redraws every cell.
    pub fn set_size(&mut self, cols: u16, rows: u16) -> Result<(), Error<W::Error>> {
        self.size = checked_size(cols, rows)?;
        Ok(())
    }

    pub fn screen_mode(&self) -> TerminalMode {
        *self.screen.current()
    }

    /// Emits the SDK transition to `mode` (alternate screen, cursor visibility,
    /// bracketed paste) and flushes. Raw/echo routing is not output state; it
    /// belongs to the session's mode lease.
    pub fn set_screen_mode(&mut self, mode: TerminalMode) -> Result<(), Error<W::Error>> {
        let switches_screen = mode.alternate_screen != self.screen.current().alternate_screen;
        self.screen.set_mode(mode, &mut self.out);
        if switches_screen {
            self.forget_position();
        }
        self.flush()
    }

    /// Emits the transition back to [`TerminalMode::CANONICAL`] and flushes.
    pub fn restore_screen_mode(&mut self) -> Result<(), Error<W::Error>> {
        self.set_screen_mode(*self.screen.baseline())
    }

    fn forget_position(&mut self) {
        self.cursor = None;
        self.pen = None;
    }

    fn move_to(&mut self, position: Position) {
        if self.cursor != Some(position) {
            self.out.extend_from_slice(b"\x1b[");
            push_decimal(&mut self.out, position.y.saturating_add(1));
            self.out.push(b';');
            push_decimal(&mut self.out, position.x.saturating_add(1));
            self.out.push(b'H');
            self.cursor = Some(position);
        }
    }

    fn set_pen(&mut self, next: Pen) {
        let current = match self.pen {
            Some(current) if current == next => return,
            current => current,
        };
        let mut sgr = Sgr::begin(&mut self.out);
        match current {
            Some(current)
                if current.modifier.difference(next.modifier).is_empty()
                    && (next.fg != Color::Reset || current.fg == Color::Reset)
                    && (next.bg != Color::Reset || current.bg == Color::Reset) =>
            {
                sgr.modifiers(next.modifier.difference(current.modifier));
                if next.fg != current.fg {
                    sgr.color(next.fg, false);
                }
                if next.bg != current.bg {
                    sgr.color(next.bg, true);
                }
            }
            _ => {
                sgr.param(0);
                sgr.modifiers(next.modifier);
                sgr.color(next.fg, false);
                sgr.color(next.bg, true);
            }
        }
        sgr.end();
        self.pen = Some(next);
    }

    fn reset_pen(&mut self) {
        if self.pen != Some(Pen::DEFAULT) {
            ScreenControls::reset_attributes(&mut self.out);
            self.pen = Some(Pen::DEFAULT);
        }
    }

    fn put_symbol(&mut self, position: Position, symbol: &str) {
        // Widgets already drop control characters; a cell set directly must
        // still never smuggle an escape sequence past the output profile.
        let width = if symbol.contains(char::is_control) {
            self.out.extend_from_slice("\u{fffd}".as_bytes());
            0
        } else {
            self.out.extend_from_slice(symbol.as_bytes());
            Span::raw(symbol).width()
        };
        let next = usize::from(position.x).saturating_add(width);
        // Width 0 is uncertain; reaching the last column leaves a pending wrap.
        self.cursor = match u16::try_from(next) {
            Ok(x) if width > 0 && x < self.size.width => Some(Position::new(x, position.y)),
            _ => None,
        };
    }

    /// Writes every complete chunk; the remainder stays buffered.
    fn write_full_chunks(&mut self) -> Result<(), Error<W::Error>> {
        let full = self.out.len() - self.out.len() % W::MAX_WRITE.max(1);
        self.write_prefix(full)
    }

    fn write_prefix(&mut self, len: usize) -> Result<(), Error<W::Error>> {
        let mut written = 0;
        while written < len {
            let end = len.min(written + W::MAX_WRITE.max(1));
            let result = match self.sink.write(&self.out[written..end]) {
                Ok(n) if n != 0 && n <= end - written => {
                    written += n;
                    continue;
                }
                Ok(n) => Err(Error::WriteCount(n)),
                Err(error) => Err(Error::Write(error)),
            };
            self.discard_output();
            return result;
        }
        self.out.drain(..len);
        if self.out.is_empty() {
            self.flushed = *self.screen.current();
        }
        Ok(())
    }

    /// After a failed write the terminal holds an unknown prefix of the
    /// buffer: forget cursor and rendition, and roll the screen mode back to
    /// the last fully written state so lost transitions are sent again
    /// (DECSET/DECRST are idempotent). Ratatui may already have swapped the
    /// failed frame in; `Terminal::clear` forces a full repaint.
    fn discard_output(&mut self) {
        self.out.clear();
        self.forget_position();
        if *self.screen.current() != self.flushed {
            let mut screen = ModeGuard::new(*self.screen.baseline());
            screen.set_mode(self.flushed, &mut self.out);
            self.out.clear();
            self.screen = screen;
        }
    }
}

impl<W: ByteSink> Backend for CellosBackend<W> {
    type Error = Error<W::Error>;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        for (x, y, cell) in content {
            let position = Position::new(x, y);
            self.move_to(position);
            self.set_pen(Pen::of(cell));
            self.put_symbol(position, cell.symbol());
            self.write_full_chunks()?;
        }
        if self.pen.is_some() {
            self.reset_pen();
        }
        Ok(())
    }

    fn append_lines(&mut self, n: u16) -> Result<(), Self::Error> {
        if n == 0 {
            Ok(())
        } else {
            Err(Error::Unsupported)
        }
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        let mode = TerminalMode {
            cursor_visible: false,
            ..*self.screen.current()
        };
        self.screen.set_mode(mode, &mut self.out);
        Ok(())
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        let mode = TerminalMode {
            cursor_visible: true,
            ..*self.screen.current()
        };
        self.screen.set_mode(mode, &mut self.out);
        Ok(())
    }

    fn get_cursor_position(&mut self) -> Result<Position, Self::Error> {
        self.cursor.ok_or(Error::CursorUnknown)
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), Self::Error> {
        self.move_to(position.into());
        Ok(())
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.clear_region(ClearType::All)
    }

    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), Self::Error> {
        // Erased cells take the current background: clear with the default pen.
        self.reset_pen();
        let sequence: &[u8] = match clear_type {
            ClearType::All => b"\x1b[2J",
            ClearType::AfterCursor => b"\x1b[J",
            ClearType::BeforeCursor => b"\x1b[1J",
            ClearType::CurrentLine => b"\x1b[2K",
            ClearType::UntilNewLine => b"\x1b[K",
        };
        self.out.extend_from_slice(sequence);
        self.write_full_chunks()
    }

    fn size(&self) -> Result<Size, Self::Error> {
        Ok(self.size)
    }

    /// Pixel dimensions are not reported by the session; zero means unknown.
    fn window_size(&mut self) -> Result<WindowSize, Self::Error> {
        Ok(WindowSize {
            columns_rows: self.size,
            pixels: Size::ZERO,
        })
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.write_prefix(self.out.len())
    }
}

impl<W: ByteSink> Drop for CellosBackend<W> {
    fn drop(&mut self) {
        self.screen.restore(&mut self.out);
        let _ = self.flush();
    }
}

fn checked_size<E>(cols: u16, rows: u16) -> Result<Size, Error<E>> {
    ViewportSize::try_new(rows, cols).map_err(|_| Error::InvalidSize { cols, rows })?;
    Ok(Size::new(cols, rows))
}

fn push_decimal(out: &mut Vec<u8>, value: u16) {
    let mut digits = [0u8; 5];
    let mut start = digits.len();
    let mut rest = value;
    loop {
        start -= 1;
        digits[start] = b'0' + (rest % 10) as u8;
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    out.extend_from_slice(&digits[start..]);
}

/// One `CSI params m` sequence; emits nothing when no parameter was added.
struct Sgr<'a> {
    out: &'a mut Vec<u8>,
    start: usize,
}

impl<'a> Sgr<'a> {
    fn begin(out: &'a mut Vec<u8>) -> Self {
        let start = out.len();
        Self { out, start }
    }

    fn param(&mut self, value: u16) {
        if self.out.len() == self.start {
            self.out.extend_from_slice(b"\x1b[");
        } else {
            self.out.push(b';');
        }
        push_decimal(self.out, value);
    }

    fn modifiers(&mut self, modifier: Modifier) {
        for (flag, code) in [
            (Modifier::BOLD, 1),
            (Modifier::DIM, 2),
            (Modifier::ITALIC, 3),
            (Modifier::UNDERLINED, 4),
            (Modifier::REVERSED, 7),
        ] {
            if modifier.contains(flag) {
                self.param(code);
            }
        }
    }

    /// `Color::Reset` adds nothing: callers reach it only through SGR 0.
    fn color(&mut self, color: Color, background: bool) {
        let offset = if background { 10 } else { 0 };
        let code = match color {
            Color::Reset => return,
            Color::Black => 30,
            Color::Red => 31,
            Color::Green => 32,
            Color::Yellow => 33,
            Color::Blue => 34,
            Color::Magenta => 35,
            Color::Cyan => 36,
            Color::Gray => 37,
            Color::DarkGray => 90,
            Color::LightRed => 91,
            Color::LightGreen => 92,
            Color::LightYellow => 93,
            Color::LightBlue => 94,
            Color::LightMagenta => 95,
            Color::LightCyan => 96,
            Color::White => 97,
            Color::Indexed(index) => {
                self.param(38 + offset);
                self.param(5);
                self.param(index.into());
                return;
            }
            Color::Rgb(r, g, b) => {
                self.param(38 + offset);
                self.param(2);
                self.param(r.into());
                self.param(g.into());
                self.param(b.into());
                return;
            }
        };
        self.param(code + offset);
    }

    fn end(self) {
        if self.out.len() != self.start {
            self.out.push(b'm');
        }
    }
}
