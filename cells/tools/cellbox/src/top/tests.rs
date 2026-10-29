//! Host tests of the interactive `top` view: the same snapshot through the TUI, batch
//! text and records; the key reducer; drill-down; geometry; state rendering; the
//! seed-capture exit path. Snapshots are constructed with `api::telemetry` (D64-14).

extern crate std;

use super::tui::{self, color_mode, run_with, Outcome, Source, MAX_VIEW};
use super::view::{Notice, RowId, Step, ViewState, INTERRUPTED};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use api::cell_session::frontend::{FRONTEND_KIND_GUI, FRONTEND_KIND_SERIAL, FRONTEND_KIND_SSH};
use api::telemetry::{
    cell_flags, cell_state, encode_name, header_flags, task_flags, task_state, TelemetryCellV1,
    TelemetryHeaderV1, TelemetrySnapshot, TelemetryTaskV1, SCOPE_SYSTEM, TELEMETRY_VERSION,
};
use terminal_sdk::codec::{KeyState, KeySymbol, Modifiers};
use text_engine::pipeline::{Item, Value};
use text_engine::top::records::frame_items;
use text_engine::top::sample::{Frame, Rows, Sampler};
use text_engine::top::text::{self, TopError};
use text_engine::top::{parse_options, Field, SortSpec, TopOptions, View};
use vitui::event::{Event, Key};
use vitui::ratatui::core::buffer::Buffer;
use vitui::ratatui::core::layout::Rect;
use vitui::ratatui::core::style::Color;
use vitui::theme::{ColorMode, Theme};

const EPOCH: u64 = 0x5eed_0f_70b;
const SEC: u64 = 1_000_000_000;
const MS: u64 = 1_000_000;

fn header(sample_id: u64, at_ns: u64) -> TelemetryHeaderV1 {
    TelemetryHeaderV1 {
        version: TELEMETRY_VERSION,
        flags: header_flags::COMPLETE | header_flags::CPU_AVAILABLE | header_flags::MEMORY_INTERVAL,
        provider_epoch: EPOCH,
        sample_id,
        capture_start_ns: at_ns,
        capture_end_ns: at_ns + 1_000,
        online_cpus: 1,
        scope: SCOPE_SYSTEM,
        ..TelemetryHeaderV1::default()
    }
}

fn cell(cell_id: u64, generation: u64, cpu_ns: u64, name: &str) -> TelemetryCellV1 {
    TelemetryCellV1 {
        cell_id,
        generation,
        root_tid: cell_id * 10,
        cpu_ns,
        quota_charged_bytes: 96 * 1024,
        quota_limit_bytes: 8 * 1024 * 1024,
        task_count: 1,
        state: cell_state::LIVE,
        flags: cell_flags::CPU_AVAILABLE | cell_flags::QUOTA_AVAILABLE | cell_flags::LIMIT_AVAILABLE,
        reserved: 0,
        name: encode_name(name).0,
    }
}

fn task(tid: u64, cell_id: u64, generation: u64, cpu_ns: u64, name: &str) -> TelemetryTaskV1 {
    TelemetryTaskV1 {
        tid,
        cell_id,
        generation,
        cpu_ns,
        user_stack_bytes: 8 * 1024,
        segment_bytes: 256 * 1024,
        state: task_state::RUNNING,
        flags: task_flags::CPU_AVAILABLE,
        name: encode_name(name).0,
    }
}

fn encode(mut head: TelemetryHeaderV1, cells: &[TelemetryCellV1], tasks: &[TelemetryTaskV1]) -> Vec<u8> {
    head.emitted_cells = cells.len() as u32;
    head.emitted_tasks = tasks.len() as u32;
    head.total_cells = head.total_cells.max(head.emitted_cells);
    head.total_tasks = head.total_tasks.max(head.emitted_tasks);
    let mut bytes = Vec::from(head.to_bytes());
    cells.iter().for_each(|row| bytes.extend_from_slice(&row.to_bytes()));
    tasks.iter().for_each(|row| bytes.extend_from_slice(&row.to_bytes()));
    bytes
}

fn observe(sampler: &mut Sampler, bytes: &[u8], view: View) -> Frame {
    sampler.observe(&TelemetrySnapshot::parse(bytes).expect("valid snapshot"), view)
}

fn opts(args: &[&str]) -> TopOptions {
    parse_options(args).expect("valid options")
}

/// A before/after pair whose second frame has Ok, New and Reset rows of both kinds.
fn pair() -> (Vec<u8>, Vec<u8>) {
    let mut first = header(1, 10 * SEC);
    first.flags |= header_flags::SYSMEM_PRESENT;
    first.mem_total_frames = 8192;
    first.mem_used_frames = 2048;
    first.mem_free_frames = 6144;
    first.mem_page_size = 4096;
    let mut second = first;
    second.sample_id = 2;
    second.capture_start_ns = 11 * SEC;
    second.capture_end_ns = 11 * SEC + 1_000;
    let before = encode(
        first,
        &[cell(1, 1, 2 * SEC, "init"), cell(2, 4, 0, "cellbox"), cell(3, 8, 5 * SEC, "busy")],
        &[task(10, 1, 1, 2 * SEC, "init"), task(20, 2, 4, 0, "cellbox"), task(30, 3, 8, 5 * SEC, "busy")],
    );
    let after = encode(
        second,
        &[cell(1, 1, 2 * SEC + 250 * MS, "init"), cell(2, 4, 0, "cellbox"), cell(3, 8, 4 * SEC, "busy"), cell(5, 12, MS, "fresh")],
        &[
            task(10, 1, 1, 2 * SEC + 250 * MS, "init"),
            task(20, 2, 4, 0, "cellbox"),
            task(30, 3, 8, 4 * SEC, "busy"),
            task(50, 5, 12, MS, "fresh"),
        ],
    );
    (before, after)
}

fn pair_frame(view: View) -> Frame {
    let (before, after) = pair();
    let mut sampler = Sampler::new();
    observe(&mut sampler, &before, view);
    observe(&mut sampler, &after, view)
}

/// A Cells frame of `count` incarnations (ids 1..=count), every one new.
fn many_cells(count: u64, sample_id: u64) -> Frame {
    let cells: Vec<TelemetryCellV1> = (1..=count).map(|id| cell(id, id, id * MS, "c")).collect();
    observe(&mut Sampler::new(), &encode(header(sample_id, SEC), &cells, &[]), View::Cells)
}

fn theme() -> Theme {
    Theme::new(ColorMode::TrueColor)
}

fn draw(state: &mut ViewState, frame: &Frame, cols: u16, rows: u16) -> Buffer {
    let area = Rect::new(0, 0, cols, rows);
    let mut buf = Buffer::empty(area);
    state.draw(frame, &theme(), area, false, &mut buf);
    buf
}

fn line(buf: &Buffer, y: u16, from: u16, to: u16) -> String {
    (from..to).map(|x| buf[(x, y)].symbol()).collect()
}

fn key(symbol: KeySymbol) -> Event {
    Event::Key(Key { symbol, modifiers: Modifiers::NONE, state: KeyState::Press })
}

fn typed(text: &str) -> Event {
    Event::Text(text.to_string())
}

/// Data rows of a drawn table (below the header row), as whitespace tokens, from the
/// column after the highlight symbol to the right border.
fn drawn_rows(buf: &Buffer, cols: u16, rows: u16) -> Vec<Vec<String>> {
    (4..rows - 2)
        .map(|y| line(buf, y, 3, cols - 1).split_whitespace().map(String::from).collect::<Vec<_>>())
        .filter(|tokens: &Vec<String>| !tokens.is_empty())
        .collect()
}

fn batch_rows(frame: &Frame) -> Vec<Vec<String>> {
    let mut out = String::new();
    text::render_frame(frame, None, &mut out);
    out.lines().skip(2).map(|line| line.split_whitespace().map(String::from).collect()).collect()
}

fn field<'a>(record: &'a Value, name: &str) -> &'a Value {
    record.get_field(name).unwrap_or_else(|| panic!("missing {name}"))
}

fn uint(value: &Value) -> u64 {
    match value {
        Value::UInt(value) => *value,
        other => panic!("expected UInt, got {other:?}"),
    }
}

// ─── same snapshot: TUI, batch text and records ─────────────────────────────

#[test]
fn one_snapshot_gives_identical_numbers_in_the_tui_batch_text_and_records() {
    for (view, args) in [(View::Cells, &["-a"][..]), (View::Tasks, &["-a", "--tasks"][..])] {
        let mut frame = pair_frame(view);
        let mut state = ViewState::new(&opts(args));
        state.prepare(&mut frame);
        let buf = draw(&mut state, &frame, 79, 26);
        let shown = drawn_rows(&buf, 79, 26);
        assert_eq!(shown, batch_rows(&frame), "{view:?}: TUI cells equal batch text");
        let records: Vec<Value> = frame_items(frame.clone())
            .filter_map(|item| match item {
                Item::Row(row) => Some(row),
                _ => None,
            })
            .collect();
        assert_eq!(shown.len(), records.len());
        let mut qualities = Vec::new();
        for (row, record) in shown.iter().zip(&records) {
            let identity = match view {
                View::Cells => format!("{}:{}", uint(field(record, "cell_id")), uint(field(record, "generation"))),
                _ => uint(field(record, "tid")).to_string(),
            };
            assert_eq!(row[0], identity);
            assert_eq!(Value::String(row[1].clone()), *field(record, "name"));
            let quality = field(record, "cpu_quality").clone();
            let cpu = &row[3];
            match (field(record, "cpu_tenths"), &quality) {
                (Value::UInt(tenths), _) => assert_eq!(*cpu, format!("{}.{}", tenths / 10, tenths % 10)),
                (Value::Null, Value::String(label)) if label == "new" || label == "reset" => assert_eq!(cpu, label),
                other => panic!("{other:?}"),
            }
            qualities.push(quality);
        }
        for label in ["ok", "new", "reset"] {
            assert!(qualities.contains(&Value::String(label.into())), "{view:?} lacks {label}");
        }
    }
}

// ─── reducer ────────────────────────────────────────────────────────────────

#[test]
fn arrows_and_pages_move_the_selection_within_the_rows() {
    let mut frame = many_cells(30, 1);
    let mut state = ViewState::new(&opts(&[]));
    state.prepare(&mut frame);
    draw(&mut state, &frame, 79, 26);
    let id = |index: usize| match &frame.rows {
        Rows::Cells(rows) => Some(RowId::Cell { cell_id: rows[index].cell_id, generation: rows[index].generation }),
        Rows::Tasks(_) => None,
    };
    assert_eq!(state.selected(), id(0));
    assert_eq!(state.on_event(&key(KeySymbol::Up), &frame), Step::Redraw);
    assert_eq!(state.selected(), id(0), "Up clamps at the first row");
    state.on_event(&key(KeySymbol::Down), &frame);
    assert_eq!(state.selected(), id(1));
    state.on_event(&key(KeySymbol::PageDown), &frame);
    assert_eq!(state.selected(), id(21), "one page is the 20 visible rows at 79x26");
    state.on_event(&key(KeySymbol::PageDown), &frame);
    assert_eq!(state.selected(), id(29), "PageDown clamps at the last row");
    state.on_event(&key(KeySymbol::Down), &frame);
    assert_eq!(state.selected(), id(29));
    state.on_event(&key(KeySymbol::PageUp), &frame);
    assert_eq!(state.selected(), id(9));
    // The selected row stays visible: the highlight symbol is on its line.
    let buf = draw(&mut state, &frame, 79, 26);
    let marked: Vec<u16> = (4..24).filter(|&y| buf[(1, y)].symbol() == "\u{258c}").collect();
    assert_eq!(marked.len(), 1);
}

#[test]
fn selection_follows_its_row_across_sorts_and_frames() {
    let mut frame = pair_frame(View::Cells);
    let mut state = ViewState::new(&opts(&[]));
    state.prepare(&mut frame);
    state.on_event(&key(KeySymbol::Down), &frame);
    let chosen = state.selected().expect("a row is selected");
    assert_eq!(state.on_event(&typed("r"), &frame), Step::Redraw);
    state.resort(&mut frame);
    assert_eq!(state.selected(), Some(chosen), "a re-sort keeps the same row");
    // A new frame of the same incarnations: still the same row.
    let (before, after) = pair();
    let mut sampler = Sampler::new();
    observe(&mut sampler, &before, View::Cells);
    observe(&mut sampler, &after, View::Cells);
    let mut next = observe(&mut sampler, &after, View::Cells);
    state.prepare(&mut next);
    assert_eq!(state.selected(), Some(chosen));
    // The selected row disappears: the selection keeps its index.
    let index = match &next.rows {
        Rows::Cells(rows) => rows.iter().position(|row| RowId::Cell { cell_id: row.cell_id, generation: row.generation } == chosen),
        Rows::Tasks(_) => None,
    }
    .unwrap();
    let mut gone = next.clone();
    if let Rows::Cells(rows) = &mut gone.rows {
        rows.remove(index);
    }
    state.resort(&mut gone);
    let expected = match &gone.rows {
        Rows::Cells(rows) => rows.get(index.min(rows.len() - 1)).map(|row| RowId::Cell { cell_id: row.cell_id, generation: row.generation }),
        Rows::Tasks(_) => None,
    };
    assert_eq!(state.selected(), expected);
}

#[test]
fn enter_drills_into_the_incarnation_and_escape_returns_to_cells() {
    let mut frame = pair_frame(View::Cells);
    let mut state = ViewState::new(&opts(&["-o", "cell_id"]));
    state.prepare(&mut frame);
    state.on_event(&key(KeySymbol::Down), &frame);
    let Some(RowId::Cell { cell_id, generation }) = state.selected() else { panic!("a Cell row") };
    assert_eq!(state.on_event(&key(KeySymbol::Enter), &frame), Step::Recapture);
    assert_eq!(state.view(), View::Cell { cell_id, generation });
    // Enter on a task row does nothing.
    let mut tasks = pair_frame(state.view());
    state.prepare(&mut tasks);
    assert!(matches!(&tasks.rows, Rows::Tasks(rows) if !rows.is_empty()
        && rows.iter().all(|row| row.cell_id == Some(cell_id) && row.generation == Some(generation))));
    assert_eq!(state.on_event(&key(KeySymbol::Enter), &tasks), Step::Idle);
    assert_eq!(state.on_event(&key(KeySymbol::Escape), &tasks), Step::Recapture);
    assert_eq!(state.view(), View::Cells);
    let mut cells = pair_frame(View::Cells);
    state.prepare(&mut cells);
    assert_eq!(state.selected(), Some(RowId::Cell { cell_id, generation }), "the drilled Cell stays selected");
    // Backspace also returns; in the Cells view Esc does nothing.
    assert_eq!(state.on_event(&key(KeySymbol::Escape), &cells), Step::Idle);
    let mut task_view = ViewState::new(&opts(&["--tasks"]));
    let mut all = pair_frame(View::Tasks);
    task_view.prepare(&mut all);
    assert_eq!(task_view.on_event(&key(KeySymbol::Backspace), &all), Step::Recapture);
    assert_eq!(task_view.view(), View::Cells);
}

#[test]
fn o_cycles_the_shown_columns_and_r_reverses() {
    let frame = pair_frame(View::Cells);
    let mut state = ViewState::new(&opts(&[]));
    let shown: Vec<Field> = text::columns(View::Cells.row_kind(), None).into_iter().map(|c| c.field()).collect();
    let start = shown.iter().position(|&f| f == state.sort().field).unwrap();
    for step in 1..=shown.len() {
        assert_eq!(state.on_event(&typed("o"), &frame), Step::Redraw);
        assert_eq!(state.sort(), SortSpec::new(shown[(start + step) % shown.len()]));
    }
    let before = state.sort();
    state.on_event(&typed("r"), &frame);
    assert_eq!(state.sort().field, before.field);
    assert_eq!(state.sort().descending, !before.descending);
    // Columns chosen with --fields are the cycle.
    let mut picked = ViewState::new(&opts(&["--fields", "name,tasks"]));
    picked.on_event(&typed("o"), &frame);
    assert_eq!(picked.sort(), SortSpec::new(Field::Name));
    picked.on_event(&typed("o"), &frame);
    assert_eq!(picked.sort(), SortSpec::new(Field::Tasks));
}

#[test]
fn the_field_picker_yields_the_fields_option() {
    let frame = pair_frame(View::Cells);
    let mut state = ViewState::new(&opts(&[]));
    assert_eq!(state.on_event(&typed("f"), &frame), Step::Redraw);
    // The cursor starts on cell_id, checked because CELL shows it: uncheck it, then check
    // generation, the next field.
    state.on_event(&typed(" "), &frame);
    state.on_event(&key(KeySymbol::Down), &frame);
    state.on_event(&typed(" "), &frame);
    assert_eq!(state.on_event(&key(KeySymbol::Enter), &frame), Step::Redraw);
    let expected = opts(&["--fields", "generation,name,state,tasks,cpu_tenths,quota_bytes,limit_bytes"]).fields;
    assert_eq!(state.fields(), expected.as_deref());
    // One toggle changes the column count by exactly one.
    state.on_event(&typed("f"), &frame);
    state.on_event(&typed(" "), &frame);
    state.on_event(&key(KeySymbol::Enter), &frame);
    assert_eq!(state.fields().map(<[Field]>::len), Some(8), "cell_id checked again: 7 + 1");
    // Esc cancels; keys inside the picker never move the table or sort.
    let sort = state.sort();
    state.on_event(&typed("f"), &frame);
    state.on_event(&typed("o"), &frame);
    state.on_event(&typed(" "), &frame);
    state.on_event(&key(KeySymbol::Escape), &frame);
    assert_eq!(state.fields().map(<[Field]>::len), Some(8));
    assert_eq!(state.sort(), sort);
    // Unchecking everything falls back to the default columns.
    let mut empty = ViewState::new(&opts(&["--fields", "name"]));
    empty.on_event(&typed("f"), &frame);
    for _ in 0..3 {
        empty.on_event(&key(KeySymbol::Down), &frame);
    }
    empty.on_event(&typed(" "), &frame);
    empty.on_event(&key(KeySymbol::Enter), &frame);
    assert_eq!(empty.fields(), None);
}

#[test]
fn quit_keys_map_to_exit_statuses_and_nothing_else_acts() {
    let mut frame = pair_frame(View::Cells);
    let mut state = ViewState::new(&opts(&[]));
    state.prepare(&mut frame);
    assert_eq!(state.on_event(&typed("q"), &frame), Step::Quit(0));
    assert_eq!(state.on_event(&typed("Q"), &frame), Step::Quit(0));
    assert_eq!(state.on_event(&typed("rq"), &frame), Step::Quit(0));
    assert_eq!(state.on_event(&Event::Interrupt, &frame), Step::Quit(INTERRUPTED));
    assert_eq!(INTERRUPTED, 130);
    assert_eq!(state.on_event(&Event::Eof, &frame), Step::Quit(0));
    // No lifecycle key exists: kill-like keys and pastes are inert.
    let (view, selected, sort) = (state.view(), state.selected(), state.sort());
    for event in [typed("k"), typed("K"), key(KeySymbol::Delete), Event::Paste("q\no\nk".to_string())] {
        assert_eq!(state.on_event(&event, &frame), Step::Idle);
    }
    let released = Event::Key(Key { symbol: KeySymbol::Down, modifiers: Modifiers::NONE, state: KeyState::Release });
    assert_eq!(state.on_event(&released, &frame), Step::Idle);
    assert_eq!((state.view(), state.selected(), state.sort()), (view, selected, sort));
}

// ─── drill-down to a gone incarnation ───────────────────────────────────────

#[test]
fn a_gone_incarnation_shows_a_notice_and_never_the_new_generation() {
    let mut state = ViewState::new(&opts(&["--cell", "3:8"]));
    // Cell 3 was relaunched: generation 9 now owns task 31.
    let bytes = encode(
        header(3, 12 * SEC),
        &[cell(1, 1, 0, "init"), cell(3, 9, 0, "busy")],
        &[task(10, 1, 1, 0, "init"), task(31, 3, 9, 0, "busy")],
    );
    let mut frame = observe(&mut Sampler::new(), &bytes, state.view());
    state.prepare(&mut frame);
    assert_eq!(state.notice(), Some(Notice::Gone));
    assert_eq!(frame.row_count(), 0, "no row of generation 9");
    let buf = draw(&mut state, &frame, 79, 26);
    assert!(drawn_rows(&buf, 79, 26).iter().all(|row| !row.contains(&"31".to_string())));
    // An incomplete capture cannot tell: no notice.
    let mut cut = header(4, 13 * SEC);
    cut.flags &= !header_flags::COMPLETE;
    cut.total_tasks = 9;
    let mut partial = observe(&mut Sampler::new(), &encode(cut, &[cell(1, 1, 0, "init")], &[task(10, 1, 1, 0, "init")]), state.view());
    state.prepare(&mut partial);
    assert_eq!(state.notice(), None);
}

// ─── geometry ───────────────────────────────────────────────────────────────

#[test]
fn every_row_of_the_frame_reaches_the_last_column_and_bars_fill_their_rows() {
    let theme = theme();
    for (cols, rows, data_rows) in [(79u16, 26u16, 20usize), (80, 24, 18), (40, 12, 6), (120, 40, 34)] {
        let mut frame = many_cells(60, 1);
        let mut state = ViewState::new(&opts(&[]));
        state.prepare(&mut frame);
        let buf = draw(&mut state, &frame, cols, rows);
        for y in 2..=rows - 2 {
            assert_ne!(buf[(cols - 1, y)].symbol(), " ", "{cols}x{rows}: border at row {y}");
            assert_ne!(buf[(0, y)].symbol(), " ", "{cols}x{rows}: left border at row {y}");
        }
        for x in 0..cols {
            assert_eq!(buf[(x, 0)].bg, theme.color(theme.palette.accent), "title bar at {x}");
            assert_eq!(buf[(x, 1)].bg, theme.color(theme.palette.surface), "status line at {x}");
            assert_eq!(buf[(x, rows - 1)].bg, theme.color(theme.palette.surface), "key hints at {x}");
        }
        assert_eq!(drawn_rows(&buf, cols, rows).len(), data_rows, "{cols}x{rows}");
        // The highlight band covers the selected (first) row only.
        assert_eq!(buf[(1, 4)].symbol(), "\u{258c}");
        assert_eq!(buf[(cols - 2, 4)].bg, theme.color(theme.palette.highlight));
        assert_ne!(buf[(cols - 2, 5)].bg, theme.color(theme.palette.highlight));
    }
}

#[test]
fn the_picker_and_a_clamped_view_stay_inside_their_area() {
    for (cols, rows) in [(40u16, 12u16), (79, 26)] {
        let frame = pair_frame(View::Cells);
        let mut state = ViewState::new(&opts(&[]));
        state.on_event(&typed("f"), &frame);
        let outer = Rect::new(0, 0, cols + 10, rows + 6);
        let mut buf = Buffer::empty(outer);
        state.draw(&frame, &theme(), Rect::new(0, 0, cols, rows), false, &mut buf);
        for y in 0..outer.height {
            for x in 0..outer.width {
                if x >= cols || y >= rows {
                    assert_eq!(buf[(x, y)], Buffer::empty(Rect::new(0, 0, 1, 1))[(0, 0)], "{x},{y} outside");
                }
            }
        }
        // The picker lists the fields with the first one under the cursor.
        assert!((2..rows - 1).any(|y| line(&buf, y, 0, cols).contains("[x] cell_id")));
    }
    // A session above the cap draws only the capped area; nothing outside is touched.
    assert_eq!(usize::from(MAX_VIEW.width) * usize::from(MAX_VIEW.height), 4_800);
    let frame = many_cells(60, 1);
    let mut state = ViewState::new(&opts(&[]));
    let session = Rect::new(0, 0, 132, 50);
    let mut buf = Buffer::empty(session);
    let area = Rect::new(0, 0, MAX_VIEW.width, MAX_VIEW.height);
    state.draw(&frame, &theme(), area, true, &mut buf);
    assert_ne!(buf[(119, 20)].symbol(), " ");
    for x in 120..132 {
        assert_eq!(buf[(x, 20)].symbol(), " ");
        assert_eq!(buf[(x, 20)].bg, Color::Reset);
    }
    assert_eq!(buf[(0, 45)].bg, Color::Reset);
}

// ─── rendering of states ────────────────────────────────────────────────────

/// The x range of `token` (whitespace-delimited) on row `y`.
fn token_span(buf: &Buffer, y: u16, cols: u16, token: &str) -> Option<core::ops::Range<u16>> {
    let text = line(buf, y, 0, cols);
    let mut x = 0usize;
    for word in text.split(' ') {
        if word == token {
            return Some(x as u16..(x + word.chars().count()) as u16);
        }
        x += word.chars().count() + 1;
    }
    None
}

#[test]
fn truncation_unavailable_cpu_and_denied_memory_are_marked_never_zero() {
    let theme = theme();
    let mut cut = header(7, SEC);
    cut.flags &= !(header_flags::COMPLETE | header_flags::CPU_AVAILABLE);
    cut.flags |= header_flags::SYSMEM_DENIED;
    cut.total_cells = 40;
    let mut unclocked = cell(1, 1, 0, "a");
    unclocked.flags &= !cell_flags::CPU_AVAILABLE;
    let bytes = encode(cut, &[unclocked, cell(2, 2, 0, "b")], &[]);
    let mut sampler = Sampler::new();
    observe(&mut sampler, &bytes, View::Cells);
    let mut frame = observe(&mut sampler, &bytes, View::Cells);
    let mut state = ViewState::new(&opts(&[]));
    state.prepare(&mut frame);
    let buf = draw(&mut state, &frame, 79, 26);
    let dropped = token_span(&buf, 1, 79, "38").expect("the dropped count is on the status line");
    for x in dropped {
        assert_eq!(buf[(x, 1)].fg, theme.color(theme.palette.warning));
    }
    let denied = token_span(&buf, 1, 79, &text::header_parts(&frame)[3].text).expect("memory state");
    for x in denied {
        assert_eq!(buf[(x, 1)].fg, theme.color(theme.palette.muted));
    }
    for row in drawn_rows(&buf, 79, 26) {
        assert!(!row.contains(&"0.0".to_string()), "{row:?}");
        assert_eq!(row[3], text::format_cpu(&text_engine::top::sample::Cpu {
            ns: None,
            tenths: None,
            quality: text_engine::top::sample::Quality::Unavailable,
        }));
    }
}

// ─── exit paths and selection ───────────────────────────────────────────────

struct Fixed(Result<(), TopError>, usize);

impl Source for Fixed {
    fn capture(&mut self, _view: View) -> Result<Frame, TopError> {
        self.1 += 1;
        self.0.map(|()| many_cells(1, self.1 as u64))
    }
}

#[test]
fn a_denied_seed_capture_never_reaches_the_terminal() {
    let options = opts(&[]);
    let mut entered = false;
    let outcome = run_with(&options, &mut Fixed(Err(TopError::Denied), 0), |_, _| {
        entered = true;
        Outcome { status: 0, diagnostic: None }
    });
    assert!(!entered, "no screen bytes: the terminal step never ran");
    assert_eq!(outcome, Outcome { status: 1, diagnostic: Some(TopError::Denied.message("top")) });
    // A good seed reaches the terminal step once, with the requested view.
    let mut source = Fixed(Ok(()), 0);
    let outcome = run_with(&opts(&["--tasks"]), &mut source, |source, view| {
        assert_eq!(source.1, 1);
        Outcome { status: if view.view() == View::Tasks { 0 } else { 9 }, diagnostic: None }
    });
    assert_eq!(outcome.status, 0);
}

#[test]
fn interactive_top_refuses_without_a_terminal_or_below_the_delay_floor() {
    // Host tests have no session: refused before any capture, status 2, one diagnostic.
    for args in [&[][..], &["-d", "0.05"], &["--tasks", "-n", "1"]] {
        let outcome = tui::run(&opts(args));
        assert_eq!(outcome.status, 2, "{args:?}");
        assert!(outcome.diagnostic.as_deref().is_some_and(|text| text.contains("-b")), "{args:?}");
    }
}

#[test]
fn colour_mode_is_truecolor_only_on_the_gui_terminal() {
    assert_eq!(color_mode(FRONTEND_KIND_GUI), ColorMode::TrueColor);
    for frontend in [FRONTEND_KIND_SERIAL, FRONTEND_KIND_SSH, 7] {
        assert_eq!(color_mode(frontend), ColorMode::Ansi16);
    }
}

/// S10-3b: the interruptible wait over an injected clock (one tick = 10 ns here).
#[test]
fn wait_core_elapses_at_the_deadline_and_stops_at_the_first_interrupt() {
    use super::{wait_core, Waited};
    use core::cell::Cell;
    let clock = Cell::new(0u64);
    let parks = Cell::new(0u32);
    let polls = Cell::new(0u32);
    let mut now = || Some(clock.get());
    let mut park = || {
        parks.set(parks.get() + 1);
        clock.set(clock.get() + 10);
    };
    // No interrupt: parks until the deadline (100 ns = 10 ticks).
    assert_eq!(wait_core(100, &mut now, &mut park, &mut || false), Waited::Elapsed);
    assert_eq!(parks.get(), 10);
    // Interrupt on the 4th poll: no park after it.
    clock.set(0);
    parks.set(0);
    let mut poll = || {
        polls.set(polls.get() + 1);
        polls.get() == 4
    };
    assert_eq!(wait_core(100, &mut now, &mut park, &mut poll), Waited::Interrupted);
    assert_eq!((polls.get(), parks.get()), (4, 3));
    // An interrupt already pending is seen before the first park, even for `sleep 0`.
    assert_eq!(wait_core(0, &mut now, &mut park, &mut || true), Waited::Interrupted);
    assert_eq!(wait_core(0, &mut now, &mut park, &mut || false), Waited::Elapsed);
    // No clock: nothing to wait against, as before.
    assert_eq!(wait_core(100, &mut || None, &mut || panic!("parked"), &mut || false), Waited::Elapsed);
}
