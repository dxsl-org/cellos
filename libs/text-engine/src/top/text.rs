//! The one text table model shared by batch `top`, the interactive view and text `ps`.
//!
//! It prints the same sample values the records carry. A CPU rate that is not
//! [`Quality::Ok`] is printed as a marker, never as `0.0`. Every batch line fits
//! [`MAX_WIDTH`] columns: an over-wide table first narrows the name column and
//! then omits trailing columns whole ([`widths`]). The interactive view lays out
//! the same [`Column`]s, labels and cell texts with its own width.

use super::sample::{CellSample, Cpu, Frame, Quality, Rows, SysMem, TaskSample};
use super::{Field, RowKind};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use api::telemetry::TelemetryDecodeError;
use core::fmt::Write;

/// Widest line the batch renderer emits (the 79-column GUI grid).
pub const MAX_WIDTH: usize = 79;
const MIN_NAME_WIDTH: usize = 4;
/// Space between two columns.
pub const GAP: &str = "  ";
/// Printed for a Null value.
const NULL_TEXT: &str = "-";

/// Why a `top`/`ps` capture produced no frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TopError {
    /// The provider refused the caller (`PermissionDenied`).
    Denied,
    OutOfMemory,
    /// The snapshot bytes did not decode.
    Malformed,
    /// Rows were dropped and the consumer cannot carry truncation.
    Incomplete { emitted: u64, total: u64 },
}

impl TopError {
    /// One diagnostic line (no newline), prefixed with `command`.
    pub fn message(&self, command: &str) -> String {
        match self {
            TopError::Denied => format!("{command}: telemetry denied"),
            TopError::OutOfMemory => format!("{command}: out of memory reading telemetry"),
            TopError::Malformed => format!("{command}: malformed telemetry snapshot"),
            TopError::Incomplete { emitted, total } => {
                format!("{command}: telemetry incomplete: {emitted} of {total} rows captured")
            }
        }
    }

    /// Exit status: 1 for every telemetry failure (usage errors are 2).
    pub fn status(&self) -> i32 {
        1
    }
}

impl From<TelemetryDecodeError> for TopError {
    fn from(_: TelemetryDecodeError) -> Self {
        TopError::Malformed
    }
}

fn push_bytes(out: &mut String, bytes: u64) {
    const UNITS: [&str; 7] = ["B", "K", "M", "G", "T", "P", "E"];
    if bytes < 1024 {
        let _ = write!(out, "{bytes}B");
        return;
    }
    let mut unit = 0usize;
    let mut scale = 1u128;
    while u128::from(bytes) >= scale * 1024 && unit + 1 < UNITS.len() {
        scale *= 1024;
        unit += 1;
    }
    let tenths = u128::from(bytes) * 10 / scale;
    let _ = write!(out, "{}.{}{}", tenths / 10, tenths % 10, UNITS[unit]);
}

/// Binary units with one floored decimal: `512B`, `1.5K`, `12.0M`.
pub fn format_bytes(bytes: u64) -> String {
    let mut out = String::new();
    push_bytes(&mut out, bytes);
    out
}

fn push_cpu(out: &mut String, cpu: &Cpu) {
    match (cpu.quality, cpu.tenths) {
        (Quality::Ok, Some(tenths)) => {
            let _ = write!(out, "{}.{}", tenths / 10, tenths % 10);
        }
        (Quality::New, _) => out.push_str("new"),
        (Quality::Reset, _) => out.push_str("reset"),
        (Quality::Ok | Quality::Unavailable, _) => out.push_str("n/a"),
    }
}

/// The CPU% cell: tenths as a percent for an Ok sample, else the quality marker.
pub fn format_cpu(cpu: &Cpu) -> String {
    let mut out = String::new();
    push_cpu(&mut out, cpu);
    out
}

/// One table column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Column {
    /// `cell_id:generation` of a Cell row (D64-4 `CELL`).
    CellIdentity,
    Field(Field),
}

const CELL_COLUMNS: [Column; 7] = [
    Column::CellIdentity,
    Column::Field(Field::Name),
    Column::Field(Field::Tasks),
    Column::Field(Field::CpuTenths),
    Column::Field(Field::QuotaBytes),
    Column::Field(Field::LimitBytes),
    Column::Field(Field::State),
];

const TASK_COLUMNS: [Column; 6] = [
    Column::Field(Field::Tid),
    Column::Field(Field::Name),
    Column::Field(Field::State),
    Column::Field(Field::CpuTenths),
    Column::Field(Field::StackBytes),
    Column::Field(Field::SegmentBytes),
];

impl Column {
    /// The column heading.
    pub fn label(self) -> String {
        let field = match self {
            Column::CellIdentity => return "CELL".to_string(),
            Column::Field(field) => field,
        };
        let label = match field {
            Field::CpuTenths => "CPU%",
            Field::QuotaBytes => "QUOTA",
            Field::LimitBytes => "LIMIT",
            Field::StackBytes => "STACK",
            Field::SegmentBytes => "SEGMENTS",
            other => return other.name().to_ascii_uppercase(),
        };
        label.to_string()
    }

    /// Numbers are right-aligned; names, states and flags left-aligned.
    pub fn right_aligned(self) -> bool {
        !matches!(
            self,
            Column::Field(
                Field::Name | Field::State | Field::CpuQuality | Field::NameTruncated | Field::StackInQuota
            )
        )
    }

    /// The record field this column shows; `CELL` stands for `cell_id`.
    pub fn field(self) -> Field {
        match self {
            Column::CellIdentity => Field::CellId,
            Column::Field(field) => field,
        }
    }
}

/// The columns of a `kind` table: the named record fields of that kind, in the
/// given order (others are skipped), or the D64-4 defaults when `fields` is
/// `None` or names nothing applicable.
pub fn columns(kind: RowKind, fields: Option<&[Field]>) -> Vec<Column> {
    let chosen: Vec<Column> = fields
        .unwrap_or(&[])
        .iter()
        .filter(|field| field.applies_to(kind))
        .map(|&field| Column::Field(field))
        .collect();
    if !chosen.is_empty() {
        return chosen;
    }
    match kind {
        RowKind::Cells => CELL_COLUMNS.to_vec(),
        RowKind::Tasks => TASK_COLUMNS.to_vec(),
    }
}

fn push_opt(out: &mut String, value: Option<u64>, push: fn(&mut String, u64)) {
    match value {
        Some(value) => push(out, value),
        None => out.push_str(NULL_TEXT),
    }
}

fn push_decimal(out: &mut String, value: u64) {
    let _ = write!(out, "{value}");
}

fn push_flag(out: &mut String, value: bool) {
    out.push_str(if value { "yes" } else { "no" });
}

fn push_cpu_field(out: &mut String, cpu: &Cpu, field: Field) {
    match field {
        Field::CpuNs => push_opt(out, cpu.ns, push_decimal),
        Field::CpuQuality => out.push_str(cpu.quality.label()),
        _ => push_cpu(out, cpu),
    }
}

fn push_cell(out: &mut String, row: &CellSample, column: Column) {
    let field = match column {
        Column::CellIdentity => {
            let _ = write!(out, "{}:{}", row.cell_id, row.generation);
            return;
        }
        Column::Field(field) => field,
    };
    match field {
        Field::CellId => push_decimal(out, row.cell_id),
        Field::Generation => push_decimal(out, row.generation),
        Field::RootTid => push_decimal(out, row.root_tid),
        Field::Name => out.push_str(&row.name),
        Field::NameTruncated => push_flag(out, row.name_truncated),
        Field::State => out.push_str(row.state.label()),
        Field::Tasks => push_decimal(out, u64::from(row.tasks)),
        Field::CpuNs | Field::CpuTenths | Field::CpuQuality => push_cpu_field(out, &row.cpu, field),
        Field::QuotaBytes => push_opt(out, row.quota_bytes, push_bytes),
        Field::LimitBytes => push_opt(out, row.limit_bytes, push_bytes),
        Field::Tid | Field::StackBytes | Field::SegmentBytes | Field::StackInQuota => out.push_str(NULL_TEXT),
    }
}

fn push_task(out: &mut String, row: &TaskSample, column: Column) {
    let field = match column {
        Column::CellIdentity => {
            out.push_str(NULL_TEXT);
            return;
        }
        Column::Field(field) => field,
    };
    match field {
        Field::Tid => push_decimal(out, row.tid),
        Field::CellId => push_opt(out, row.cell_id, push_decimal),
        Field::Generation => push_opt(out, row.generation, push_decimal),
        Field::Name => out.push_str(&row.name),
        Field::NameTruncated => push_flag(out, row.name_truncated),
        Field::State => out.push_str(row.state.label()),
        Field::CpuNs | Field::CpuTenths | Field::CpuQuality => push_cpu_field(out, &row.cpu, field),
        Field::StackBytes => push_bytes(out, row.stack_bytes),
        Field::SegmentBytes => push_bytes(out, row.segment_bytes),
        Field::StackInQuota => push_flag(out, row.stack_in_quota),
        Field::RootTid | Field::Tasks | Field::QuotaBytes | Field::LimitBytes => out.push_str(NULL_TEXT),
    }
}

/// The text of one Cell row's `column`.
pub fn cell_text(row: &CellSample, column: Column) -> String {
    let mut out = String::new();
    push_cell(&mut out, row, column);
    out
}

/// The text of one task row's `column`.
pub fn task_text(row: &TaskSample, column: Column) -> String {
    let mut out = String::new();
    push_task(&mut out, row, column);
    out
}

fn width(text: &str) -> usize {
    text.chars().count()
}

/// The first `max` characters of `text`.
fn clip(text: &str, max: usize) -> &str {
    match text.char_indices().nth(max) {
        Some((end, _)) => &text[..end],
        None => text,
    }
}

/// What one piece of the header line reports, so a view can mark it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderMark {
    Plain,
    /// `TRUNCATED n`: rows were dropped.
    Truncated,
    /// System memory `denied` or `n/a`.
    Unavailable,
}

/// One piece of the header line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderPart {
    pub text: String,
    pub mark: HeaderMark,
}

/// The header line in pieces: sample id, `complete` or `TRUNCATED n`, the CPU
/// unit and system memory (`used/total`, `denied` or `n/a`).
pub fn header_parts(frame: &Frame) -> Vec<HeaderPart> {
    let part = |text: String, mark| HeaderPart { text, mark };
    let status = if frame.complete {
        part("complete".to_string(), HeaderMark::Plain)
    } else {
        part(format!("TRUNCATED {}", frame.dropped), HeaderMark::Truncated)
    };
    let mem = match frame.meta.mem {
        SysMem::Present { total_bytes, used_bytes, .. } => {
            part(format!("{}/{}", format_bytes(used_bytes), format_bytes(total_bytes)), HeaderMark::Plain)
        }
        SysMem::Denied => part("denied".to_string(), HeaderMark::Unavailable),
        SysMem::Absent => part("n/a".to_string(), HeaderMark::Unavailable),
    };
    alloc::vec![
        part(format!("sample {}  ", frame.meta.sample_id), HeaderMark::Plain),
        status,
        part("  cpu% per core  mem ".to_string(), HeaderMark::Plain),
        mem,
    ]
}

/// The header line ([`header_parts`] joined), unclipped.
pub fn header_text(frame: &Frame) -> String {
    header_parts(frame).into_iter().map(|part| part.text).collect()
}

/// Column widths for `columns` of `frame` within `max_width`: each column is as
/// wide as its label and its widest cell over all rows; an over-wide table drops
/// trailing columns that cannot fit even with the name column at its minimum,
/// then narrows the name column only as far as needed. The result may be
/// shorter than `columns` (dropped columns). Cell texts are formatted into one
/// reused buffer; nothing is retained.
pub fn widths(frame: &Frame, columns: &[Column], max_width: usize) -> Vec<usize> {
    let mut widths: Vec<usize> = columns.iter().map(|column| width(&column.label())).collect();
    let mut text = String::new();
    let mut widen = |widths: &mut Vec<usize>, push: &mut dyn FnMut(&mut String, Column)| {
        for (index, &column) in columns.iter().enumerate() {
            text.clear();
            push(&mut text, column);
            widths[index] = widths[index].max(width(&text));
        }
    };
    match &frame.rows {
        Rows::Cells(rows) => rows.iter().for_each(|row| widen(&mut widths, &mut |out, column| push_cell(out, row, column))),
        Rows::Tasks(rows) => rows.iter().for_each(|row| widen(&mut widths, &mut |out, column| push_task(out, row, column))),
    }
    fit_widths(columns, &mut widths, max_width);
    widths
}

/// Render `frame` as one header line and a table.
///
/// `fields` names record fields of the frame's row kind (others are skipped);
/// `None` (or nothing applicable) uses the D64-4 default columns ([`columns`]).
pub fn render_frame(frame: &Frame, fields: Option<&[Field]>, out: &mut String) {
    out.push_str(clip(&header_text(frame), MAX_WIDTH));
    out.push('\n');
    let chosen = columns(frame.row_kind(), fields);
    let widths = widths(frame, &chosen, MAX_WIDTH);
    let mut line = String::new();
    let mut cell = String::new();
    push_line(out, &mut line, &mut cell, &chosen, &widths, &mut |text, column| text.push_str(&column.label()));
    match &frame.rows {
        Rows::Cells(rows) => rows.iter().for_each(|row| {
            push_line(out, &mut line, &mut cell, &chosen, &widths, &mut |text, column| push_cell(text, row, column))
        }),
        Rows::Tasks(rows) => rows.iter().for_each(|row| {
            push_line(out, &mut line, &mut cell, &chosen, &widths, &mut |text, column| push_task(text, row, column))
        }),
    }
}

/// One table line: every kept column clipped to its width and padded by its
/// alignment, joined by [`GAP`], trailing blanks trimmed.
fn push_line(
    out: &mut String,
    line: &mut String,
    cell: &mut String,
    columns: &[Column],
    widths: &[usize],
    push: &mut dyn FnMut(&mut String, Column),
) {
    line.clear();
    for (index, &column_width) in widths.iter().enumerate() {
        if index > 0 {
            line.push_str(GAP);
        }
        cell.clear();
        push(cell, columns[index]);
        let text = clip(cell, column_width);
        let pad = column_width - width(text);
        let last = index + 1 == widths.len();
        if columns[index].right_aligned() {
            line.extend(core::iter::repeat(' ').take(pad));
            line.push_str(text);
        } else {
            line.push_str(text);
            if !last {
                line.extend(core::iter::repeat(' ').take(pad));
            }
        }
    }
    out.push_str(clip(line.trim_end(), MAX_WIDTH));
    out.push('\n');
}

/// Fit a line into `max_width`: drop trailing columns that cannot fit even
/// with the name column at its minimum, then narrow the name column only as
/// far as needed.
fn fit_widths(columns: &[Column], widths: &mut Vec<usize>, max_width: usize) {
    let total = |widths: &[usize]| widths.iter().sum::<usize>() + GAP.len() * widths.len().saturating_sub(1);
    let name = columns.iter().position(|&column| column == Column::Field(Field::Name));
    let narrowest = |widths: &[usize]| match name {
        Some(name) if name < widths.len() => total(widths) - widths[name] + widths[name].min(MIN_NAME_WIDTH),
        _ => total(widths),
    };
    while widths.len() > 1 && narrowest(widths) > max_width {
        widths.pop();
    }
    if let Some(name) = name.filter(|&name| name < widths.len()) {
        let over = total(widths).saturating_sub(max_width);
        widths[name] -= over.min(widths[name].saturating_sub(MIN_NAME_WIDTH));
    }
}
