//! Pure state and drawing of the interactive `top` view (no syscalls; host-tested).
//!
//! [`ViewState`] turns input events into a [`Step`] for the session loop (`tui.rs`) and
//! draws one frame into a Ratatui buffer. Every number, unit and quality marker on
//! screen comes from `text_engine::top::text`, the code that prints batch text. The
//! reducer has no lifecycle action: observing grants no right over what is observed.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use terminal_sdk::codec::{KeyState, KeySymbol};
use text_engine::top::sample::{Frame, Rows};
use text_engine::top::text::{self, Column, HeaderMark};
use text_engine::top::{Field, RowKind, SortSpec, TopOptions, View};
use vitui::event::{Event, Key};
use vitui::ratatui::core::buffer::Buffer;
use vitui::ratatui::core::layout::{Constraint, Rect};
use vitui::ratatui::core::style::Style;
use vitui::ratatui::core::text::{Line, Span};
use vitui::ratatui::core::widgets::{StatefulWidget, Widget};
use vitui::ratatui::widgets::clear::Clear;
use vitui::ratatui::widgets::list::{List, ListItem, ListState};
use vitui::ratatui::widgets::paragraph::Paragraph;
use vitui::ratatui::widgets::table::{Cell, HighlightSpacing, Row, Table, TableState};
use vitui::theme::{Theme, HIGHLIGHT_SYMBOL};

/// Exit status of a run ended by Ctrl-C (shell convention).
pub const INTERRUPTED: i32 = 130;

/// Width of the always-reserved highlight column ([`HIGHLIGHT_SYMBOL`]).
const HIGHLIGHT_WIDTH: u16 = 2;
/// Rows above the body (title bar, status line) and below it (key hints).
const BARS_ABOVE: u16 = 2;
const BARS_BELOW: u16 = 1;
const PICKER_WIDTH: u16 = 26;

const KEY_HINTS: [(&str, &str); 7] = [
    ("q", "quit"),
    ("Up/Dn", "select"),
    ("Enter", "tasks"),
    ("Esc", "back"),
    ("o", "sort"),
    ("r", "reverse"),
    ("f", "fields"),
];

/// What the session loop does after an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Nothing changed.
    Idle,
    /// Re-sort the current frame and draw it again (no capture).
    Redraw,
    /// The view changed: capture now and draw.
    Recapture,
    /// Leave the view with this exit status.
    Quit(i32),
}

/// Identity of one row: a Cell incarnation or a task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowId {
    Cell { cell_id: u64, generation: u64 },
    Task { tid: u64 },
}

/// A message shown in the body instead of rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notice {
    /// The drilled incarnation is in no task row of a complete capture: it exited.
    Gone,
    /// The view has no rows.
    Empty,
}

/// The field picker: every record field of the row kind with its toggle state.
#[derive(Clone, Debug, PartialEq, Eq)]
struct FieldPicker {
    kind: RowKind,
    checked: Vec<bool>,
    cursor: usize,
}

/// The interactive view: what is shown, how it is sorted, which row is selected.
#[derive(Clone, Debug)]
pub struct ViewState {
    view: View,
    /// Root name of the drilled Cell when it was entered (title only).
    cell_name: String,
    /// Sort per row kind ([`kind_index`]).
    sort: [SortSpec; 2],
    /// `--fields` per row kind; `None` shows the default columns.
    fields: [Option<Vec<Field>>; 2],
    selected: Option<RowId>,
    index: usize,
    offset: usize,
    /// Data rows visible at the last draw (PageUp/PageDown step).
    page: usize,
    picker: Option<FieldPicker>,
    notice: Option<Notice>,
    /// The Cell to select again when a drill-down returns to the Cells view.
    return_to: Option<RowId>,
    show_all: bool,
    delay_ns: u64,
}

fn kind_index(kind: RowKind) -> usize {
    match kind {
        RowKind::Cells => 0,
        RowKind::Tasks => 1,
    }
}

fn row_id(frame: &Frame, index: usize) -> Option<RowId> {
    match &frame.rows {
        Rows::Cells(rows) => rows.get(index).map(|row| RowId::Cell { cell_id: row.cell_id, generation: row.generation }),
        Rows::Tasks(rows) => rows.get(index).map(|row| RowId::Task { tid: row.tid }),
    }
}

fn position(frame: &Frame, id: RowId) -> Option<usize> {
    (0..frame.row_count()).find(|&index| row_id(frame, index) == Some(id))
}

fn has_incarnation(frame: &Frame, cell_id: u64, generation: u64) -> bool {
    match &frame.rows {
        Rows::Tasks(rows) => rows.iter().any(|row| row.cell_id == Some(cell_id) && row.generation == Some(generation)),
        Rows::Cells(rows) => rows.iter().any(|row| row.cell_id == cell_id && row.generation == generation),
    }
}

fn pressed(key: &Key) -> bool {
    matches!(key.state, KeyState::Press | KeyState::Repeat) && key.modifiers.is_empty()
}

/// Decimal seconds with the trailing zeros of the fraction dropped (`1`, `0.5`, `2.25`).
fn seconds(ns: u64) -> String {
    let (whole, fraction) = (ns / 1_000_000_000, ns % 1_000_000_000);
    if fraction == 0 {
        return format!("{whole}");
    }
    let digits = format!("{fraction:09}");
    format!("{whole}.{}", digits.trim_end_matches('0'))
}

impl ViewState {
    pub fn new(options: &TopOptions) -> Self {
        let kind = kind_index(options.view.row_kind());
        let mut sort = [SortSpec::default_for(View::Cells), SortSpec::default_for(View::Tasks)];
        sort[kind] = options.sort;
        let mut fields = [None, None];
        fields[kind] = options.fields.clone();
        Self {
            view: options.view,
            cell_name: String::new(),
            sort,
            fields,
            selected: None,
            index: 0,
            offset: 0,
            page: 1,
            picker: None,
            notice: None,
            return_to: None,
            show_all: options.show_all,
            delay_ns: options.delay_ns,
        }
    }

    pub fn view(&self) -> View {
        self.view
    }

    /// The sort of the current view.
    pub fn sort(&self) -> SortSpec {
        self.sort[kind_index(self.view.row_kind())]
    }

    /// `--fields` of the current view.
    #[cfg(test)]
    pub fn fields(&self) -> Option<&[Field]> {
        self.fields[kind_index(self.view.row_kind())].as_deref()
    }

    #[cfg(test)]
    pub fn selected(&self) -> Option<RowId> {
        self.selected
    }

    #[cfg(test)]
    pub fn notice(&self) -> Option<Notice> {
        self.notice
    }

    /// Prepare a fresh capture of the current view: note a drilled incarnation that is
    /// gone (only a complete capture can tell), hide what the view does not show, sort,
    /// and keep the selection on the same row.
    pub fn prepare(&mut self, frame: &mut Frame) {
        self.notice = match self.view {
            View::Cell { cell_id, generation } if frame.complete && !has_incarnation(frame, cell_id, generation) => {
                Some(Notice::Gone)
            }
            _ => None,
        };
        frame.retain(self.show_all, self.view);
        self.resort(frame);
    }

    /// Sort `frame` by the current sort and follow the selected row.
    pub fn resort(&mut self, frame: &mut Frame) {
        frame.sort(&self.sort());
        self.follow(frame);
    }

    /// The selection follows its row's identity; a row that disappeared leaves the
    /// selection at its index (clamped).
    fn follow(&mut self, frame: &Frame) {
        let len = frame.row_count();
        if len == 0 {
            self.index = 0;
            self.selected = None;
            return;
        }
        if let Some(index) = self.selected.and_then(|id| position(frame, id)) {
            self.index = index;
        }
        self.index = self.index.min(len - 1);
        self.selected = row_id(frame, self.index);
    }

    /// The step for one input event, given the frame on screen. Typed text is handled
    /// character by character; a paste never acts as keys.
    pub fn on_event(&mut self, event: &Event, frame: &Frame) -> Step {
        match event {
            Event::Interrupt => Step::Quit(INTERRUPTED),
            Event::Eof => Step::Quit(0),
            Event::Text(text) => {
                let mut step = Step::Idle;
                for character in text.chars() {
                    match self.on_char(character) {
                        Step::Idle => {}
                        Step::Quit(status) => return Step::Quit(status),
                        other => step = other,
                    }
                }
                step
            }
            Event::Key(key) if pressed(key) => self.on_key(key.symbol, frame),
            _ => Step::Idle,
        }
    }

    fn on_char(&mut self, character: char) -> Step {
        if character == 'q' || character == 'Q' {
            return Step::Quit(0);
        }
        if let Some(picker) = &mut self.picker {
            if character == ' ' {
                picker.checked[picker.cursor] = !picker.checked[picker.cursor];
                return Step::Redraw;
            }
            return Step::Idle;
        }
        match character {
            'o' => self.cycle_sort(),
            'r' => {
                let sort = &mut self.sort[kind_index(self.view.row_kind())];
                sort.descending = !sort.descending;
                Step::Redraw
            }
            'f' => {
                self.open_picker();
                Step::Redraw
            }
            _ => Step::Idle,
        }
    }

    fn on_key(&mut self, symbol: KeySymbol, frame: &Frame) -> Step {
        let page = self.page.max(1) as isize;
        if let Some(picker) = &mut self.picker {
            let last = picker.checked.len() - 1;
            let mut moved = |delta: isize| {
                picker.cursor = picker.cursor.saturating_add_signed(delta).min(last);
                Step::Redraw
            };
            return match symbol {
                KeySymbol::Up => moved(-1),
                KeySymbol::Down => moved(1),
                KeySymbol::PageUp => moved(-page),
                KeySymbol::PageDown => moved(page),
                KeySymbol::Enter => {
                    self.apply_picker();
                    Step::Redraw
                }
                KeySymbol::Escape | KeySymbol::Backspace => {
                    self.picker = None;
                    Step::Redraw
                }
                _ => Step::Idle,
            };
        }
        match symbol {
            KeySymbol::Up => self.move_selection(-1, frame),
            KeySymbol::Down => self.move_selection(1, frame),
            KeySymbol::PageUp => self.move_selection(-page, frame),
            KeySymbol::PageDown => self.move_selection(page, frame),
            KeySymbol::Enter => self.drill(frame),
            KeySymbol::Escape | KeySymbol::Backspace => self.back(),
            _ => Step::Idle,
        }
    }

    fn move_selection(&mut self, delta: isize, frame: &Frame) -> Step {
        let len = frame.row_count();
        if len == 0 {
            return Step::Idle;
        }
        self.index = self.index.saturating_add_signed(delta).min(len - 1);
        self.selected = row_id(frame, self.index);
        Step::Redraw
    }

    /// Enter on a Cell row: that incarnation's tasks, by identity (never by name).
    fn drill(&mut self, frame: &Frame) -> Step {
        let Rows::Cells(rows) = &frame.rows else {
            return Step::Idle;
        };
        let Some(row) = rows.get(self.index) else {
            return Step::Idle;
        };
        self.view = View::Cell { cell_id: row.cell_id, generation: row.generation };
        self.cell_name = row.name.clone();
        self.return_to = self.selected;
        self.reset_selection();
        Step::Recapture
    }

    /// Esc/Backspace in a task view: back to the Cells view, the drilled Cell selected.
    fn back(&mut self) -> Step {
        if self.view.row_kind() != RowKind::Tasks {
            return Step::Idle;
        }
        self.view = View::Cells;
        self.reset_selection();
        self.selected = self.return_to.take();
        Step::Recapture
    }

    fn reset_selection(&mut self) {
        self.selected = None;
        self.index = 0;
        self.offset = 0;
        self.notice = None;
    }

    fn columns(&self, kind: RowKind) -> Vec<Column> {
        text::columns(kind, self.fields[kind_index(kind)].as_deref())
    }

    /// `o`: the next shown column after the current sort column, in its natural direction.
    fn cycle_sort(&mut self) -> Step {
        let kind = self.view.row_kind();
        let shown: Vec<Field> = self.columns(kind).into_iter().map(Column::field).collect();
        let sort = &mut self.sort[kind_index(kind)];
        let next = match shown.iter().position(|&field| field == sort.field) {
            Some(index) => shown[(index + 1) % shown.len()],
            None => shown[0],
        };
        *sort = SortSpec::new(next);
        Step::Redraw
    }

    fn open_picker(&mut self) {
        let kind = self.view.row_kind();
        let shown: Vec<Field> = self.columns(kind).into_iter().map(Column::field).collect();
        let checked = kind.fields().iter().map(|field| shown.contains(field)).collect();
        self.picker = Some(FieldPicker { kind, checked, cursor: 0 });
    }

    /// The checked fields in record order, exactly as `--fields` would name them; an
    /// empty set falls back to the default columns.
    fn apply_picker(&mut self) {
        let Some(picker) = self.picker.take() else {
            return;
        };
        let fields: Vec<Field> = picker
            .kind
            .fields()
            .iter()
            .zip(&picker.checked)
            .filter(|(_, &checked)| checked)
            .map(|(&field, _)| field)
            .collect();
        self.fields[kind_index(picker.kind)] = (!fields.is_empty()).then_some(fields);
    }

    fn title(&self) -> String {
        match self.view {
            View::Cells => String::from("Cells"),
            View::Tasks => String::from("Tasks"),
            View::Cell { cell_id, generation } => format!("Cell {cell_id}:{generation} {}", self.cell_name),
        }
    }

    /// Draw `frame` into `area` of `buf`: title bar, status line, the bordered table
    /// over the full width, key hints; the field picker on top when open. `clamped`
    /// says the session is larger than `area` (the size cap).
    pub fn draw(&mut self, frame: &Frame, theme: &Theme, area: Rect, clamped: bool, buf: &mut Buffer) {
        buf.set_style(area, theme.base());
        if area.height <= BARS_ABOVE + BARS_BELOW || area.width == 0 {
            return;
        }
        let row = |y: u16| Rect::new(area.x, y, area.width, 1);
        let title = self.title();
        let sort = self.sort();
        let bar = format!(
            " top  {title}  sort {} {}  every {}s",
            sort.field.name(),
            if sort.descending { "desc" } else { "asc" },
            seconds(self.delay_ns)
        );
        Paragraph::new(theme.title_bar(&bar)).style(theme.title_bar_style()).render(row(area.y), buf);
        self.draw_status(frame, theme, area, clamped, row(area.y + 1), buf);
        Paragraph::new(theme.key_hints(&KEY_HINTS))
            .style(theme.status_line_style())
            .render(row(area.bottom() - 1), buf);
        let body = Rect::new(area.x, area.y + BARS_ABOVE, area.width, area.height - BARS_ABOVE - BARS_BELOW);
        let block = theme.block(&title);
        let inner = block.inner(body);
        block.render(body, buf);
        self.draw_table(frame, theme, inner, buf);
        if self.picker.is_some() {
            self.draw_picker(theme, body, buf);
        }
    }

    fn draw_status(&self, frame: &Frame, theme: &Theme, area: Rect, clamped: bool, line: Rect, buf: &mut Buffer) {
        let warning = Style::new().fg(theme.color(theme.palette.warning));
        let mut spans: Vec<Span> = text::header_parts(frame)
            .into_iter()
            .map(|part| match part.mark {
                HeaderMark::Plain => Span::raw(part.text),
                HeaderMark::Truncated => Span::styled(part.text, warning),
                HeaderMark::Unavailable => Span::styled(part.text, theme.muted_style()),
            })
            .collect();
        spans.insert(0, Span::raw(" "));
        if clamped {
            spans.push(Span::styled(format!("  clamped {}x{}", area.width, area.height), warning));
        }
        Paragraph::new(Line::from(spans)).style(theme.status_line_style()).render(line, buf);
    }

    fn draw_table(&mut self, frame: &Frame, theme: &Theme, inner: Rect, buf: &mut Buffer) {
        if inner.height == 0 || inner.width <= HIGHLIGHT_WIDTH {
            return;
        }
        let kind = frame.row_kind();
        let columns = self.columns(kind);
        let widths = text::widths(frame, &columns, usize::from(inner.width - HIGHLIGHT_WIDTH));
        let columns = &columns[..widths.len()];
        // One header line; the rest are data rows.
        self.page = usize::from(inner.height - 1);
        let len = frame.row_count();
        if self.index < self.offset {
            self.offset = self.index;
        } else if self.page > 0 && self.index >= self.offset + self.page {
            self.offset = self.index + 1 - self.page;
        }
        self.offset = self.offset.min(len.saturating_sub(self.page));
        let visible = self.offset..len.min(self.offset + self.page);
        let cell = |text: String, column: Column| {
            let line = Line::from(text);
            Cell::from(if column.right_aligned() { line.right_aligned() } else { line })
        };
        let rows: Vec<Row> = match &frame.rows {
            Rows::Cells(rows) => rows[visible.clone()]
                .iter()
                .map(|row| columns.iter().map(|&column| cell(text::cell_text(row, column), column)).collect())
                .collect(),
            Rows::Tasks(rows) => rows[visible.clone()]
                .iter()
                .map(|row| columns.iter().map(|&column| cell(text::task_text(row, column), column)).collect())
                .collect(),
        };
        let header: Row = columns.iter().map(|&column| cell(column.label(), column)).collect();
        let constraints = widths.iter().map(|&width| Constraint::Length(width as u16));
        let table = Table::new(rows, constraints)
            .header(header.style(theme.title_style()))
            .column_spacing(text::GAP.len() as u16)
            .row_highlight_style(theme.highlight_style())
            .highlight_symbol(HIGHLIGHT_SYMBOL)
            .highlight_spacing(HighlightSpacing::Always);
        let selected = (len > 0).then(|| self.index - self.offset);
        let mut state = TableState::default().with_selected(selected);
        StatefulWidget::render(table, inner, buf, &mut state);
        let notice = match (self.notice, len) {
            (Some(notice), _) => Some(notice),
            (None, 0) => Some(Notice::Empty),
            _ => None,
        };
        if let (Some(notice), true) = (notice, inner.height > 1) {
            let text = match (notice, self.view) {
                (Notice::Gone, View::Cell { cell_id, generation }) => {
                    format!("Cell {cell_id}:{generation} has exited; Esc returns to the Cells view")
                }
                _ => String::from("no rows"),
            };
            let line = Rect::new(inner.x + HIGHLIGHT_WIDTH, inner.y + 1, inner.width - HIGHLIGHT_WIDTH, 1);
            Paragraph::new(text).style(Style::new().fg(theme.color(theme.palette.warning))).render(line, buf);
        }
    }

    fn draw_picker(&mut self, theme: &Theme, body: Rect, buf: &mut Buffer) {
        let Some(picker) = &self.picker else {
            return;
        };
        let fields = picker.kind.fields();
        let width = PICKER_WIDTH.min(body.width);
        let height = (fields.len() as u16 + 2).min(body.height);
        let area = Rect::new(
            body.x + (body.width - width) / 2,
            body.y + (body.height - height) / 2,
            width,
            height,
        );
        Clear.render(area, buf);
        let items: Vec<ListItem> = fields
            .iter()
            .zip(&picker.checked)
            .map(|(field, &checked)| ListItem::new(format!("[{}] {}", if checked { 'x' } else { ' ' }, field.name())))
            .collect();
        let list = List::new(items)
            .block(theme.block("fields"))
            .highlight_style(theme.highlight_style())
            .highlight_symbol(HIGHLIGHT_SYMBOL)
            .highlight_spacing(HighlightSpacing::Always);
        let mut state = ListState::default().with_selected(Some(picker.cursor));
        StatefulWidget::render(list, area, buf, &mut state);
    }
}
