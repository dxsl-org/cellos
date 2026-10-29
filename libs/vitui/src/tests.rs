// SPDX-License-Identifier: Apache-2.0 OR MIT
extern crate std;

use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;

use ratatui_core::backend::{Backend, ClearType};
use ratatui_core::buffer::Cell;
use ratatui_core::layout::{Position, Size};
use ratatui_core::style::{Color, Style};
use ratatui_core::terminal::{Frame, Terminal};
use ratatui_core::text::{Line, Span};
use ratatui_widgets::paragraph::Paragraph;
use terminal_sdk::codec::{KeyState, KeySymbol, Modifiers};
use terminal_sdk::input::EventKind;

use crate::backend::{ByteSink, CellosBackend, Error, TUI_SCREEN};
use crate::event::{Event, EventAdapter, Key};

#[derive(Default)]
struct Log {
    bytes: Vec<u8>,
    writes: Vec<usize>,
    fail: bool,
}

/// In-memory sink; the log stays readable after the backend is dropped.
#[derive(Clone, Default)]
struct Memory(Rc<RefCell<Log>>);

impl Memory {
    fn take(&self) -> String {
        let bytes = core::mem::take(&mut self.0.borrow_mut().bytes);
        String::from_utf8(bytes).unwrap()
    }
    fn fail(&self, fail: bool) {
        self.0.borrow_mut().fail = fail;
    }
}

impl ByteSink for Memory {
    type Error = &'static str;
    const MAX_WRITE: usize = 512;
    fn write(&mut self, bytes: &[u8]) -> Result<usize, Self::Error> {
        let mut log = self.0.borrow_mut();
        assert!(!bytes.is_empty() && bytes.len() <= Self::MAX_WRITE);
        if log.fail {
            return Err("broken pipe");
        }
        log.bytes.extend_from_slice(bytes);
        log.writes.push(bytes.len());
        Ok(bytes.len())
    }
}

/// 16-byte frames, of which at most 7 bytes are accepted per call.
#[derive(Clone, Default)]
struct Narrow(Memory);

impl ByteSink for Narrow {
    type Error = &'static str;
    const MAX_WRITE: usize = 16;
    fn write(&mut self, bytes: &[u8]) -> Result<usize, Self::Error> {
        assert!(bytes.len() <= Self::MAX_WRITE);
        self.0.write(&bytes[..bytes.len().min(7)])
    }
}

fn terminal(cols: u16, rows: u16) -> (Terminal<CellosBackend<Memory>>, Memory) {
    let sink = Memory::default();
    let backend = CellosBackend::new(sink.clone(), cols, rows).unwrap();
    (Terminal::new(backend).unwrap(), sink)
}

fn text(frame: &mut Frame, line: Line<'static>) {
    frame.render_widget(Paragraph::new(line), frame.area());
}

#[test]
fn first_draw_emits_only_non_blank_cells() {
    let (mut terminal, sink) = terminal(10, 3);
    terminal.draw(|f| text(f, Line::raw("hi"))).unwrap();
    assert_eq!(sink.take(), "\x1b[1;1H\x1b[0mhi\x1b[?25l");
}

#[test]
fn redraw_emits_only_changed_cells() {
    let (mut terminal, sink) = terminal(10, 3);
    terminal.draw(|f| text(f, Line::raw("hello"))).unwrap();
    sink.take();
    terminal.draw(|f| text(f, Line::raw("help!"))).unwrap();
    // Cursor is tracked: cell 4 follows cell 3 without a second CUP.
    assert_eq!(sink.take(), "\x1b[1;4Hp!");
    terminal.draw(|f| text(f, Line::raw("help!"))).unwrap();
    assert_eq!(sink.take(), "");
}

#[test]
fn style_changes_are_minimal_and_reset_after_the_frame() {
    let (mut terminal, sink) = terminal(10, 1);
    let line = Line::from(alloc::vec![
        Span::styled("A", Style::new().red().on_blue().bold()),
        Span::styled("B", Style::new().red().on_blue().bold().italic()),
        Span::raw("C"),
        Span::styled("D", Style::new().fg(Color::Indexed(200)).dim()),
        Span::styled(
            "E",
            Style::new()
                .fg(Color::Rgb(1, 2, 3))
                .bg(Color::Indexed(17))
                .dim()
        ),
        Span::styled(
            "F",
            Style::new()
                .fg(Color::White)
                .bg(Color::DarkGray)
                .underlined()
                .reversed()
        ),
    ]);
    terminal.draw(|f| text(f, line)).unwrap();
    assert_eq!(
        sink.take(),
        "\x1b[1;1H\x1b[0;1;31;44mA\x1b[3mB\x1b[0mC\x1b[2;38;5;200mD\x1b[38;2;1;2;3;48;5;17mE\
         \x1b[0;4;7;97;100mF\x1b[0m\x1b[?25l"
    );
}

#[test]
fn cursor_show_move_and_hide() {
    let (mut terminal, sink) = terminal(10, 3);
    terminal.draw(|f| text(f, Line::raw("ab"))).unwrap();
    assert_eq!(sink.take(), "\x1b[1;1H\x1b[0mab\x1b[?25l");
    terminal
        .draw(|f| {
            text(f, Line::raw("ab"));
            f.set_cursor_position((3, 1));
        })
        .unwrap();
    assert_eq!(sink.take(), "\x1b[?25h\x1b[2;4H");
    // Same position and visibility: nothing to send.
    terminal
        .draw(|f| {
            text(f, Line::raw("ab"));
            f.set_cursor_position((3, 1));
        })
        .unwrap();
    assert_eq!(sink.take(), "");
    terminal.draw(|f| text(f, Line::raw("ab"))).unwrap();
    assert_eq!(sink.take(), "\x1b[?25l");
    assert_eq!(terminal.get_cursor_position().unwrap(), Position::new(3, 1));
    // Ratatui shows a cursor it hid when the terminal is dropped.
    drop(terminal);
    assert_eq!(sink.take(), "\x1b[?25h");
}

#[test]
fn resize_clears_and_repaints_at_the_new_size() {
    let (mut terminal, sink) = terminal(10, 3);
    terminal.draw(|f| text(f, Line::raw("hi"))).unwrap();
    sink.take();
    terminal.backend_mut().set_size(12, 4).unwrap();
    let frame = terminal.draw(|f| text(f, Line::raw("hi"))).unwrap();
    assert_eq!(frame.area.as_size(), Size::new(12, 4));
    assert_eq!(sink.take(), "\x1b[2J\x1b[1;1Hhi");
    let window = terminal.backend_mut().window_size().unwrap();
    assert_eq!(
        (window.columns_rows, window.pixels),
        (Size::new(12, 4), Size::ZERO)
    );
    assert_eq!(
        terminal.backend_mut().set_size(0, 4),
        Err(Error::InvalidSize { cols: 0, rows: 4 })
    );
    assert_eq!(
        terminal.backend_mut().set_size(513, 4),
        Err(Error::InvalidSize { cols: 513, rows: 4 })
    );
    assert_eq!(terminal.size().unwrap(), Size::new(12, 4));
}

#[test]
fn clear_resets_rendition_first() {
    let (mut terminal, sink) = terminal(4, 2);
    let backend = terminal.backend_mut();
    backend.clear().unwrap();
    backend.clear_region(ClearType::AfterCursor).unwrap();
    backend.clear_region(ClearType::BeforeCursor).unwrap();
    backend.clear_region(ClearType::CurrentLine).unwrap();
    backend.clear_region(ClearType::UntilNewLine).unwrap();
    backend.flush().unwrap();
    // The initial rendition is unknown, so the first erase is preceded by SGR 0.
    assert_eq!(sink.take(), "\x1b[0m\x1b[2J\x1b[J\x1b[1J\x1b[2K\x1b[K");
}

#[test]
fn wide_character_occupies_two_cells() {
    let (mut terminal, sink) = terminal(6, 1);
    terminal.draw(|f| text(f, Line::raw("界x"))).unwrap();
    assert_eq!(sink.take(), "\x1b[1;1H\x1b[0m界x\x1b[?25l");
    terminal.draw(|f| text(f, Line::raw("界y"))).unwrap();
    assert_eq!(sink.take(), "\x1b[1;3Hy");
    terminal.draw(|f| text(f, Line::raw("abz"))).unwrap();
    assert_eq!(sink.take(), "\x1b[1;1Habz");
}

#[test]
fn control_characters_never_reach_the_terminal() {
    let (mut terminal, sink) = terminal(4, 1);
    terminal
        .draw(|f| {
            f.buffer_mut()[(0, 0)].set_symbol("\x1b");
            f.buffer_mut()[(1, 0)].set_symbol("z");
        })
        .unwrap();
    // The replacement's width is not Ratatui's, so the next cell is addressed.
    assert_eq!(sink.take(), "\x1b[1;1H\x1b[0m\u{fffd}\x1b[1;2Hz\x1b[?25l");
}

#[test]
fn last_column_leaves_the_cursor_unknown() {
    let (mut terminal, sink) = terminal(3, 2);
    terminal.draw(|f| text(f, Line::raw("abc"))).unwrap();
    assert_eq!(sink.take(), "\x1b[1;1H\x1b[0mabc\x1b[?25l");
    assert_eq!(
        terminal.backend_mut().get_cursor_position(),
        Err(Error::CursorUnknown)
    );
    terminal
        .draw(|f| {
            text(f, Line::raw("abc"));
            f.set_cursor_position((0, 1));
        })
        .unwrap();
    assert_eq!(sink.take(), "\x1b[?25h\x1b[2;1H");
}

#[test]
fn write_error_stops_the_draw_and_surfaces() {
    let sink = Memory::default();
    let mut backend = CellosBackend::new(Narrow(sink.clone()), 40, 1).unwrap();
    sink.fail(true);
    let cells: Vec<Cell> = (0..40)
        .map(|_| {
            let mut cell = Cell::EMPTY;
            cell.set_symbol("x");
            cell
        })
        .collect();
    let mut pulled = 0;
    let content = cells.iter().enumerate().map(|(x, cell)| {
        pulled += 1;
        (x as u16, 0, cell)
    });
    assert_eq!(backend.draw(content), Err(Error::Write("broken pipe")));
    // "\x1b[1;1H\x1b[0mx" is 11 bytes; the 6th cell fills the first 16-byte frame.
    assert_eq!(pulled, 6);
    assert!(sink.0.borrow().writes.is_empty());
    // Nothing stale is sent later; cursor and rendition are re-established.
    sink.fail(false);
    backend.flush().unwrap();
    assert_eq!(sink.take(), "");
    backend.draw([(5, 0, &cells[0])].into_iter()).unwrap();
    backend.flush().unwrap();
    assert_eq!(sink.take(), "\x1b[1;6H\x1b[0mx");
}

#[test]
fn terminal_draw_reports_flush_failure() {
    let (mut terminal, sink) = terminal(10, 3);
    sink.fail(true);
    assert!(matches!(
        terminal.draw(|f| text(f, Line::raw("hi"))),
        Err(Error::Write("broken pipe"))
    ));
    sink.fail(false);
    // Ratatui swapped the frame in before the failed flush, so an identical
    // frame has no diff; the lost hide is re-sent because the screen mode
    // rolled back. `clear` forces a full repaint.
    terminal.draw(|f| text(f, Line::raw("hi"))).unwrap();
    assert_eq!(sink.take(), "\x1b[?25l");
    terminal.clear().unwrap();
    terminal.draw(|f| text(f, Line::raw("hi"))).unwrap();
    assert_eq!(sink.take(), "\x1b[0m\x1b[2J\x1b[1;1Hhi");
}

#[test]
fn output_is_chunked_within_the_sink_bound() {
    let narrow = Memory::default();
    let mut chunked =
        Terminal::new(CellosBackend::new(Narrow(narrow.clone()), 20, 3).unwrap()).unwrap();
    let (mut whole, sink) = terminal(20, 3);
    let draw = |f: &mut Frame| {
        text(
            f,
            Line::from(alloc::vec![
                Span::raw("plain "),
                Span::styled("green", Style::new().green())
            ]),
        );
    };
    chunked.draw(draw).unwrap();
    whole.draw(draw).unwrap();
    let writes = narrow.0.borrow().writes.clone();
    assert!(writes.len() > 1 && writes.iter().all(|n| (1..=7).contains(n)));
    assert_eq!(narrow.take(), sink.take());
}

#[test]
fn screen_mode_uses_sdk_transitions_and_is_restored_on_drop() {
    let sink = Memory::default();
    let mut backend = CellosBackend::new(sink.clone(), 10, 3).unwrap();
    backend.set_screen_mode(TUI_SCREEN).unwrap();
    assert_eq!(sink.take(), "\x1b[?1049h\x1b[?25l\x1b[?2004h");
    assert_eq!(backend.screen_mode(), TUI_SCREEN);
    // Ratatui's hide is already in effect: no bytes.
    backend.hide_cursor().unwrap();
    backend.flush().unwrap();
    assert_eq!(sink.take(), "");
    backend.restore_screen_mode().unwrap();
    assert_eq!(sink.take(), "\x1b[?1049l\x1b[?25h\x1b[?2004l");
    backend.set_screen_mode(TUI_SCREEN).unwrap();
    sink.take();
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| text(f, Line::raw("x"))).unwrap();
    // A screen switch forgets cursor and rendition.
    assert_eq!(sink.take(), "\x1b[1;1H\x1b[0mx");
    drop(terminal);
    // Ratatui's drop shows the cursor it hid; the backend's drop restores the rest.
    assert_eq!(sink.take(), "\x1b[?25h\x1b[?1049l\x1b[?2004l");
}

#[test]
fn unsupported_and_unknown_are_errors_not_guesses() {
    let (mut terminal, _sink) = terminal(10, 3);
    let backend = terminal.backend_mut();
    assert_eq!(backend.get_cursor_position(), Err(Error::CursorUnknown));
    assert_eq!(backend.append_lines(0), Ok(()));
    assert_eq!(backend.append_lines(1), Err(Error::Unsupported));
    backend.set_cursor_position((2, 2)).unwrap();
    assert_eq!(backend.get_cursor_position(), Ok(Position::new(2, 2)));
    assert!(CellosBackend::new(Memory::default(), 80, 0).is_err());
}

#[test]
fn events_map_sdk_kinds_and_fold_pastes() {
    let mut adapter = EventAdapter::new();
    let up = KeySymbol::Up.code().unwrap();
    assert_eq!(
        adapter.map(EventKind::Key {
            code: up,
            modifiers: Modifiers::SHIFT.bits(),
            pressed: 1
        }),
        Some(Event::Key(Key {
            symbol: KeySymbol::Up,
            modifiers: Modifiers::SHIFT,
            state: KeyState::Press
        }))
    );
    assert_eq!(
        adapter.map(EventKind::Key {
            code: 1,
            modifiers: 0,
            pressed: 1
        }),
        Some(Event::Lost(1))
    );
    assert_eq!(
        adapter.map(EventKind::Text("é".into())),
        Some(Event::Text("é".into()))
    );
    assert_eq!(adapter.map(EventKind::PasteBegin), None);
    assert_eq!(adapter.map(EventKind::Text("a\n".into())), None);
    assert_eq!(adapter.map(EventKind::Text("b".into())), None);
    assert_eq!(
        adapter.map(EventKind::Resize {
            rows: 30,
            cols: 100
        }),
        Some(Event::Resize {
            cols: 100,
            rows: 30
        })
    );
    assert_eq!(
        adapter.map(EventKind::PasteEnd),
        Some(Event::Paste("a\nb".into()))
    );
    assert_eq!(adapter.map(EventKind::PasteEnd), Some(Event::Lost(1)));
    assert_eq!(adapter.map(EventKind::Interrupt), Some(Event::Interrupt));
    assert_eq!(adapter.map(EventKind::Eof), Some(Event::Eof));
    assert_eq!(adapter.map(EventKind::Lost(7)), Some(Event::Lost(7)));
    let mut paste = Event::Paste("secret".into());
    assert_eq!(std::format!("{paste:?}"), "Paste(<redacted 6 bytes>)");
    paste.scrub();
    assert_eq!(paste, Event::Paste(String::new()));
}

/// Every `CSI row;col H` in `bytes`, 1-based.
fn cursor_moves(bytes: &str) -> Vec<(u16, u16)> {
    let mut moves = Vec::new();
    for part in bytes.split("\x1b[").skip(1) {
        let Some(end) = part.find(|c: char| !(c.is_ascii_digit() || c == ';')) else { continue };
        if part[end..].starts_with('H') {
            let mut numbers = part[..end].split(';').map(|n| n.parse::<u16>().unwrap_or(1));
            moves.push((numbers.next().unwrap_or(1), numbers.next().unwrap_or(1)));
        }
    }
    moves
}

#[test]
fn bounded_viewport_never_exceeds_its_cap_and_the_backend_size_is_exact() {
    use crate::session::{apply_resize, new_terminal};
    let max = Size::new(120, 40);
    let sink = Memory::default();
    let backend = CellosBackend::new(sink.clone(), 100, 30).unwrap();
    let mut terminal = new_terminal(backend, Some(max)).unwrap();
    // Below the cap the whole session is drawn.
    let frame = terminal.draw(|f| text(f, Line::raw("x"))).unwrap();
    assert_eq!(frame.area, ratatui_core::layout::Rect::new(0, 0, 100, 30));
    sink.take();
    // The largest session: the buffers stay at the cap, the backend is exact.
    apply_resize(&mut terminal, 512, 256, Some(max)).unwrap();
    assert_eq!(terminal.size().unwrap(), Size::new(512, 256));
    assert_eq!(terminal.current_buffer_mut().content.len(), 120 * 40);
    let fill = "#".repeat(512);
    let frame = terminal
        .draw(|f| {
            let area = f.area();
            for y in 0..area.height {
                f.buffer_mut().set_string(0, y, &fill, Style::new());
            }
        })
        .unwrap();
    assert_eq!(frame.area, ratatui_core::layout::Rect::new(0, 0, 120, 40));
    assert_eq!(frame.buffer.content.len(), 120 * 40);
    let bytes = sink.take();
    // The resize cleared the screen, then exactly the capped cells were painted.
    assert!(bytes.contains("\x1b[J"), "{bytes:?}");
    assert_eq!(bytes.matches('#').count(), 120 * 40);
    assert!(cursor_moves(&bytes).iter().all(|&(row, col)| row <= 40 && col <= 120));
    // Shrinking below the cap follows the session again.
    apply_resize(&mut terminal, 80, 24, Some(max)).unwrap();
    assert_eq!(terminal.size().unwrap(), Size::new(80, 24));
    assert_eq!(terminal.current_buffer_mut().content.len(), 80 * 24);
    // An unbounded terminal is left to resize itself at the next draw.
    let (mut free, _) = self::terminal(100, 30);
    apply_resize(&mut free, 512, 256, None).unwrap();
    assert_eq!(free.size().unwrap(), Size::new(512, 256));
    assert_eq!(free.current_buffer_mut().content.len(), 100 * 30);
}

#[test]
fn bounded_entry_clears_from_home_row_by_row() {
    use crate::session::new_terminal;
    // The steps of `TuiSession::enter_bounded` after the input lease.
    let sink = Memory::default();
    let mut backend = CellosBackend::new(sink.clone(), 100, 30).unwrap();
    backend.set_screen_mode(TUI_SCREEN).unwrap();
    let mut terminal = new_terminal(backend, Some(Size::new(120, 40))).unwrap();
    terminal.clear().unwrap();
    terminal.backend_mut().flush().unwrap();
    // A fixed viewport erases to the end of the screen from column 1 of each row; the
    // first step (home, default pen after the screen switch) already clears it all.
    let mut want = String::from("\x1b[?1049h\x1b[?25l\x1b[?2004h\x1b[1;1H\x1b[0m\x1b[J");
    for row in 2..=30 {
        want.push_str(&alloc::format!("\x1b[{row};1H\x1b[J"));
    }
    assert_eq!(sink.take(), want);
}
