// SPDX-License-Identifier: MIT
//! Contract test between S5-1 and S5-2: frames drawn by Ratatui through the
//! `vitui` backend are fed, as bytes, to this emulator, and every screen cell must
//! equal the cell of Ratatui's own `Buffer` (symbol, colours, bold, inverse; dim,
//! italic and underline are not part of the screen model).

extern crate std;

use std::cell::RefCell;
use std::rc::Rc;
use std::string::String;
use std::vec::Vec;

use unicode_width::UnicodeWidthStr;
use vitui::backend::{ByteSink as BackendSink, CellosBackend, TUI_SCREEN};
use vitui::ratatui::core::buffer::Buffer;
use vitui::ratatui::core::layout::Position;
use vitui::ratatui::core::style::{Color, Modifier, Style};
use vitui::ratatui::core::terminal::{Frame, Terminal};

use crate::grid::{Color as GridColor, TerminalGrid, Width};
use crate::sink::ByteSink;

/// The backend's byte output, readable while the backend lives.
#[derive(Clone, Default)]
struct Wire(Rc<RefCell<Vec<u8>>>);

impl Wire {
    fn take(&self) -> Vec<u8> {
        core::mem::take(&mut self.0.borrow_mut())
    }
}

impl BackendSink for Wire {
    type Error = &'static str;
    const MAX_WRITE: usize = 512;
    fn write(&mut self, bytes: &[u8]) -> Result<usize, Self::Error> {
        self.0.borrow_mut().extend_from_slice(bytes);
        Ok(bytes.len())
    }
}

/// The emulator on the other end of the wire.
struct Screen {
    grid: TerminalGrid,
    sink: ByteSink,
}

impl Screen {
    fn new(cols: u16, rows: u16) -> Self {
        Self { grid: TerminalGrid::new(cols.into(), rows.into()), sink: ByteSink::new() }
    }

    /// Feed in uneven chunks so sequences are split like stream frames split them.
    fn feed(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(37) {
            self.sink.feed(&mut self.grid, chunk);
        }
        assert!(self.sink.is_ground());
    }
}

fn grid_color(color: Color) -> GridColor {
    match color {
        Color::Reset => GridColor::Default,
        Color::Black => GridColor::Indexed(0),
        Color::Red => GridColor::Indexed(1),
        Color::Green => GridColor::Indexed(2),
        Color::Yellow => GridColor::Indexed(3),
        Color::Blue => GridColor::Indexed(4),
        Color::Magenta => GridColor::Indexed(5),
        Color::Cyan => GridColor::Indexed(6),
        Color::Gray => GridColor::Indexed(7),
        Color::DarkGray => GridColor::Indexed(8),
        Color::LightRed => GridColor::Indexed(9),
        Color::LightGreen => GridColor::Indexed(10),
        Color::LightYellow => GridColor::Indexed(11),
        Color::LightBlue => GridColor::Indexed(12),
        Color::LightMagenta => GridColor::Indexed(13),
        Color::LightCyan => GridColor::Indexed(14),
        Color::White => GridColor::Indexed(15),
        Color::Indexed(n) => GridColor::Indexed(n),
        Color::Rgb(r, g, b) => GridColor::Rgb(r, g, b),
    }
}

/// Every cell of `buffer` equals the emulator's cell; trailing halves of wide symbols
/// are trailing cells on the screen.
fn assert_screen_matches(screen: &Screen, buffer: &Buffer) {
    let area = buffer.area;
    assert_eq!((screen.grid.cols, screen.grid.rows), (usize::from(area.width), usize::from(area.height)));
    for y in 0..area.height {
        let mut skip = 0;
        for x in 0..area.width {
            let cell = screen.grid.cell_at(x.into(), y.into()).unwrap();
            if skip > 0 {
                skip -= 1;
                assert_eq!(cell.width, Width::Trail, "({x},{y}) should be a trailing half");
                continue;
            }
            let expected = &buffer[(x, y)];
            let symbol: String = core::iter::once(cell.ch).chain(cell.mark).collect();
            let at = (x, y, expected.symbol());
            assert_eq!(symbol, expected.symbol(), "symbol at {at:?}");
            assert_eq!(cell.pen.fg, grid_color(expected.fg), "fg at {at:?}");
            assert_eq!(cell.pen.bg, grid_color(expected.bg), "bg at {at:?}");
            assert_eq!(cell.pen.bold, expected.modifier.contains(Modifier::BOLD), "bold at {at:?}");
            assert_eq!(cell.pen.inverse, expected.modifier.contains(Modifier::REVERSED), "inverse at {at:?}");
            let width = expected.symbol().width();
            assert_eq!(cell.width, if width == 2 { Width::Wide } else { Width::Narrow }, "width at {at:?}");
            skip = width.saturating_sub(1);
        }
    }
}

const NAMED: [Color; 17] = [
    Color::Reset,
    Color::Black,
    Color::Red,
    Color::Green,
    Color::Yellow,
    Color::Blue,
    Color::Magenta,
    Color::Cyan,
    Color::Gray,
    Color::DarkGray,
    Color::LightRed,
    Color::LightGreen,
    Color::LightYellow,
    Color::LightBlue,
    Color::LightMagenta,
    Color::LightCyan,
    Color::White,
];

/// A busy first frame: every named colour as fg and bg, indexed and RGB colours, all
/// profile modifiers, wide and combining symbols, a wide symbol in the last column and
/// text in the bottom-right cell.
fn first_frame(frame: &mut Frame) {
    let area = frame.area();
    let buf = frame.buffer_mut();
    for (i, color) in NAMED.iter().enumerate() {
        let x = i as u16 * 3;
        buf.set_string(x, 0, "fg", Style::new().fg(*color));
        buf.set_string(x, 1, "bg", Style::new().bg(*color).fg(NAMED[16 - i]));
    }
    buf.set_string(0, 2, "idx", Style::new().fg(Color::Indexed(200)).bg(Color::Indexed(17)));
    buf.set_string(4, 2, "rgb", Style::new().fg(Color::Rgb(1, 2, 3)).bg(Color::Rgb(250, 128, 0)));
    buf.set_string(8, 2, "B", Style::new().add_modifier(Modifier::BOLD));
    buf.set_string(9, 2, "R", Style::new().add_modifier(Modifier::REVERSED).fg(Color::Green));
    buf.set_string(10, 2, "DIU", Style::new().add_modifier(Modifier::DIM | Modifier::ITALIC | Modifier::UNDERLINED));
    buf.set_string(14, 2, "all", Style::new().red().on_blue().add_modifier(Modifier::BOLD | Modifier::REVERSED | Modifier::DIM));
    buf.set_string(0, 3, "wide 世界 and e\u{301}t\u{e9} ok", Style::new().yellow());
    // A wide symbol whose trailing half is the last column.
    buf.set_string(area.width - 2, 4, "界", Style::new().cyan().bold());
    buf.set_string(0, 5, "erase me later", Style::new().on_magenta());
    // Bottom-right cell: written with a pending wrap and nothing scrolls.
    buf.set_string(area.width - 3, area.height - 1, "end", Style::new().white().on_red());
}

/// A diff frame: wide replaced by narrow and the reverse, text cleared, styles changed,
/// and a visible cursor.
fn second_frame(frame: &mut Frame) {
    let area = frame.area();
    let buf = frame.buffer_mut();
    for (i, color) in NAMED.iter().enumerate() {
        let x = i as u16 * 3;
        buf.set_string(x, 0, "FG", Style::new().fg(NAMED[(i + 5) % 17]).add_modifier(Modifier::BOLD));
        buf.set_string(x, 1, "bg", Style::new().bg(*color).fg(NAMED[16 - i]));
    }
    buf.set_string(0, 3, "wide ab界d and e\u{301}t\u{e9} ok", Style::new().yellow());
    buf.set_string(area.width - 2, 4, "xy", Style::new());
    buf.set_string(0, 4, "世", Style::new().on_green());
    buf.set_string(area.width - 3, area.height - 1, "END", Style::new().white().on_red().reversed());
    frame.set_cursor_position(Position::new(5, 6));
}

#[test]
fn backend_frames_round_trip_through_the_emulator_cell_for_cell() {
    let (cols, rows) = (60, 8);
    let wire = Wire::default();
    let mut terminal = Terminal::new(CellosBackend::new(wire.clone(), cols, rows).unwrap()).unwrap();
    let mut screen = Screen::new(cols, rows);

    let expected = terminal.draw(first_frame).unwrap().buffer.clone();
    screen.feed(&wire.take());
    assert_screen_matches(&screen, &expected);
    assert!(!screen.grid.cursor_visible());

    let expected = terminal.draw(second_frame).unwrap().buffer.clone();
    let diff = wire.take();
    screen.feed(&diff);
    assert_screen_matches(&screen, &expected);
    // The cursor Ratatui placed is where the emulator shows it.
    assert!(screen.grid.cursor_visible());
    assert_eq!((screen.grid.cursor_x, screen.grid.cursor_y), (5, 6));
    // The redraw was a real diff, not a full repaint.
    assert!(!diff.windows(4).any(|w| w == b"\x1b[2J"));

    // An empty frame clears exactly what was drawn.
    let expected = terminal.draw(|_| {}).unwrap().buffer.clone();
    screen.feed(&wire.take());
    assert_screen_matches(&screen, &expected);
}

#[test]
fn a_resize_redraws_the_whole_frame_at_the_new_size() {
    let wire = Wire::default();
    let mut terminal = Terminal::new(CellosBackend::new(wire.clone(), 40, 6).unwrap()).unwrap();
    let mut screen = Screen::new(40, 6);
    terminal.draw(first_frame).unwrap();
    screen.feed(&wire.take());

    terminal.backend_mut().set_size(64, 10).unwrap();
    screen.grid.resize(64, 10).unwrap();
    let expected = terminal.draw(second_frame).unwrap().buffer.clone();
    screen.feed(&wire.take());
    assert_screen_matches(&screen, &expected);
}

#[test]
fn the_tui_screen_mode_enters_and_leaves_the_alternate_screen_around_the_frames() {
    let (cols, rows) = (50, 7);
    let mut screen = Screen::new(cols, rows);
    screen.feed(b"Cellos > tui-demo\r\n");
    let prompt: Vec<String> = (0..usize::from(rows)).map(|r| row_text(&screen.grid, r)).collect();
    let cursor = (screen.grid.cursor_x, screen.grid.cursor_y);

    let wire = Wire::default();
    {
        let mut backend = CellosBackend::new(wire.clone(), cols, rows).unwrap();
        backend.set_screen_mode(TUI_SCREEN).unwrap();
        let mut terminal = Terminal::new(backend).unwrap();
        let expected = terminal.draw(first_frame).unwrap().buffer.clone();
        screen.feed(&wire.take());
        assert!(screen.grid.alternate_active() && screen.grid.bracketed_paste());
        assert_screen_matches(&screen, &expected);
        let expected = terminal.draw(second_frame).unwrap().buffer.clone();
        screen.feed(&wire.take());
        assert_screen_matches(&screen, &expected);
        // Dropping the terminal drops the backend, which restores the baseline mode.
    }
    screen.feed(&wire.take());
    assert!(!screen.grid.alternate_active() && screen.grid.cursor_visible() && !screen.grid.bracketed_paste());
    let after: Vec<String> = (0..usize::from(rows)).map(|r| row_text(&screen.grid, r)).collect();
    assert_eq!(after, prompt);
    assert_eq!((screen.grid.cursor_x, screen.grid.cursor_y), cursor);
}

fn row_text(grid: &TerminalGrid, r: usize) -> String {
    let text: String = (0..grid.cols).map(|c| grid.cell_at(c, r).unwrap().ch).collect();
    text.trim_end().into()
}
