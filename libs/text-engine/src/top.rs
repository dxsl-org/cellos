//! Pure `top`/`ps` core over the Cell-aware telemetry provider (Phase 06 S6-2).
//!
//! The shell captures [`api::telemetry::TelemetrySnapshot`]s; this module turns
//! them into bounded, typed frames:
//!
//! - [`parse_options`]: the D64-2 command line ([`USAGE`]);
//! - [`sample`]: the [`sample::Sampler`] baseline, CPU quality and per-frame rows;
//! - [`records`]: `top.cell.v1`/`top.task.v1` records and Begin/Row/End items;
//! - [`text`]: the one text renderer every text mode shares.
//!
//! CPU is core-equivalent tenths of a percent (one busy core is 1000) and is
//! never clamped. A value the provider could not measure is Null, never 0.

pub mod records;
pub mod sample;
pub mod text;

#[cfg(test)]
mod tests;

use crate::pipeline;
use alloc::vec::Vec;
use api::telemetry::request;

/// Rows per frame of one kind (DV-11); rows beyond it make the frame incomplete.
pub const MAX_FRAME_ROWS: usize = pipeline::MAX_SORT_RECORDS;
/// Client cap on one snapshot buffer: covers 64 Cells + 2,048 tasks (186,464 B).
pub const MAX_SNAPSHOT_BYTES: usize = 192 * 1024;
/// Delay between frames when `-d` is absent.
pub const DEFAULT_DELAY_NS: u64 = 1_000_000_000;

pub const USAGE: &str =
    "Usage: top [-a] [-b] [-n COUNT] [-d SECS] [-o FIELD] [--fields LIST] [--tasks | --cell ID:GEN]";
/// `-o mem`/`-o heap` name a Cell's quota; task rows carry per-task bytes instead.
pub const USAGE_TASK_MEMORY: &str =
    "top: mem/heap sort Cells by quota; task rows sort by stack_bytes or segment_bytes";
/// Exit status for a usage error.
pub const USAGE_STATUS: i32 = 2;

const NS_PER_SEC: u64 = 1_000_000_000;
const MAX_DELAY_FRACTION_DIGITS: usize = 9;

/// Which rows a frame holds (D64-1 R1: one row kind per frame).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    /// One row per live or retiring Cell incarnation (the default).
    Cells,
    /// Every task, ownerless kernel tasks included.
    Tasks,
    /// The tasks of exactly one Cell incarnation.
    Cell { cell_id: u64, generation: u64 },
}

/// The row schema of a [`View`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    Cells,
    Tasks,
}

impl View {
    pub fn row_kind(self) -> RowKind {
        match self {
            View::Cells => RowKind::Cells,
            View::Tasks | View::Cell { .. } => RowKind::Tasks,
        }
    }
}

/// Every field of `top.cell.v1` and `top.task.v1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    CellId,
    Generation,
    RootTid,
    Tid,
    Name,
    NameTruncated,
    State,
    Tasks,
    CpuNs,
    CpuTenths,
    CpuQuality,
    QuotaBytes,
    LimitBytes,
    StackBytes,
    SegmentBytes,
    StackInQuota,
}

/// `top.cell.v1` fields in record order.
pub const CELL_FIELDS: [Field; 12] = [
    Field::CellId,
    Field::Generation,
    Field::RootTid,
    Field::Name,
    Field::NameTruncated,
    Field::State,
    Field::Tasks,
    Field::CpuNs,
    Field::CpuTenths,
    Field::CpuQuality,
    Field::QuotaBytes,
    Field::LimitBytes,
];

/// `top.task.v1` fields in record order.
pub const TASK_FIELDS: [Field; 12] = [
    Field::Tid,
    Field::CellId,
    Field::Generation,
    Field::Name,
    Field::NameTruncated,
    Field::State,
    Field::CpuNs,
    Field::CpuTenths,
    Field::CpuQuality,
    Field::StackBytes,
    Field::SegmentBytes,
    Field::StackInQuota,
];

impl RowKind {
    /// The record fields of this row kind, in record order.
    pub fn fields(self) -> &'static [Field] {
        match self {
            RowKind::Cells => &CELL_FIELDS,
            RowKind::Tasks => &TASK_FIELDS,
        }
    }

    /// The record schema name.
    pub fn schema(self) -> &'static str {
        match self {
            RowKind::Cells => "top.cell.v1",
            RowKind::Tasks => "top.task.v1",
        }
    }
}

impl Field {
    /// The record field name.
    pub fn name(self) -> &'static str {
        match self {
            Field::CellId => "cell_id",
            Field::Generation => "generation",
            Field::RootTid => "root_tid",
            Field::Tid => "tid",
            Field::Name => "name",
            Field::NameTruncated => "name_truncated",
            Field::State => "state",
            Field::Tasks => "tasks",
            Field::CpuNs => "cpu_ns",
            Field::CpuTenths => "cpu_tenths",
            Field::CpuQuality => "cpu_quality",
            Field::QuotaBytes => "quota_bytes",
            Field::LimitBytes => "limit_bytes",
            Field::StackBytes => "stack_bytes",
            Field::SegmentBytes => "segment_bytes",
            Field::StackInQuota => "stack_in_quota",
        }
    }

    /// True when `kind` records carry this field.
    pub fn applies_to(self, kind: RowKind) -> bool {
        kind.fields().contains(&self)
    }

    /// Numeric resource fields sort descending; identity and text fields ascending.
    pub fn sorts_descending(self) -> bool {
        matches!(
            self,
            Field::Tasks
                | Field::CpuNs
                | Field::CpuTenths
                | Field::QuotaBytes
                | Field::LimitBytes
                | Field::StackBytes
                | Field::SegmentBytes
        )
    }

    /// Resolve a field name or alias for `kind` rows.
    ///
    /// Aliases: `cpu` → `cpu_tenths`; `pid` → `cell_id` (Cells) or `tid` (tasks);
    /// `mem`/`heap` → `quota_bytes` (Cells only).
    pub fn resolve(token: &str, kind: RowKind) -> Result<Field, &'static str> {
        let field = match (token, kind) {
            ("cpu", _) => Field::CpuTenths,
            ("pid", RowKind::Cells) => Field::CellId,
            ("pid", RowKind::Tasks) => Field::Tid,
            ("mem" | "heap", RowKind::Cells) => Field::QuotaBytes,
            ("mem" | "heap", RowKind::Tasks) => return Err(USAGE_TASK_MEMORY),
            _ => *kind.fields().iter().find(|field| field.name() == token).ok_or(USAGE)?,
        };
        Ok(field)
    }
}

/// Row order for one frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SortSpec {
    pub field: Field,
    /// Larger values first. Null values sort last in either direction, and ties
    /// break on identity ascending (`cell_id, generation` or `tid`).
    pub descending: bool,
}

impl SortSpec {
    /// Sort by `field` in its natural direction ([`Field::sorts_descending`]).
    pub fn new(field: Field) -> SortSpec {
        SortSpec { field, descending: field.sorts_descending() }
    }

    /// Busiest rows first; both row kinds share the default.
    pub fn default_for(_view: View) -> SortSpec {
        SortSpec::new(Field::CpuTenths)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TopOptions {
    pub batch: bool,
    pub count: Option<usize>,
    pub delay_ns: u64,
    pub show_all: bool,
    pub view: View,
    pub sort: SortSpec,
    /// Columns named by `--fields`, in the given order (fields of the view's row kind).
    pub fields: Option<Vec<Field>>,
}

/// Parse `top` flags (D64-2).
///
/// # Errors
/// Returns usage text (exit [`USAGE_STATUS`]) for an unknown, repeated or
/// conflicting flag, or a missing or invalid flag value.
pub fn parse_options(args: &[&str]) -> Result<TopOptions, &'static str> {
    let mut batch = None;
    let mut show_all = None;
    let mut count = None;
    let mut delay_ns = None;
    let mut view = None;
    let mut sort = None;
    let mut fields = None;
    let mut words = args.iter().copied();
    while let Some(arg) = words.next() {
        match arg {
            "-a" => set_once(&mut show_all, Some(()))?,
            "-b" => set_once(&mut batch, Some(()))?,
            "-n" => set_once(
                &mut count,
                words.next().and_then(parse_decimal).and_then(|n| usize::try_from(n).ok()),
            )?,
            "-d" => set_once(&mut delay_ns, words.next().and_then(parse_delay_ns))?,
            "-o" => set_once(&mut sort, words.next())?,
            "--fields" => set_once(&mut fields, words.next())?,
            "--tasks" => set_once(&mut view, Some(View::Tasks))?,
            "--cell" => set_once(&mut view, words.next().and_then(parse_cell))?,
            _ => return Err(USAGE),
        }
    }
    let view = view.unwrap_or(View::Cells);
    let kind = view.row_kind();
    let sort = match sort {
        Some(token) => SortSpec::new(Field::resolve(token, kind)?),
        None => SortSpec::default_for(view),
    };
    let fields = fields.map(|list| parse_field_list(list, kind)).transpose()?;
    Ok(TopOptions {
        batch: batch.is_some(),
        count,
        delay_ns: delay_ns.unwrap_or(DEFAULT_DELAY_NS),
        show_all: show_all.is_some(),
        view,
        sort,
        fields,
    })
}

/// `TelemetrySnapshot` request flags for `view`: the Cells view requests no task rows.
pub fn request_flags(view: View, sysmem: bool) -> usize {
    let tasks = match view.row_kind() {
        RowKind::Cells => 0,
        RowKind::Tasks => request::TASKS,
    };
    tasks | if sysmem { request::SYSTEM_MEMORY } else { 0 }
}

fn set_once<T>(slot: &mut Option<T>, value: Option<T>) -> Result<(), &'static str> {
    if slot.is_some() {
        return Err(USAGE);
    }
    *slot = Some(value.ok_or(USAGE)?);
    Ok(())
}

fn parse_decimal(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// Decimal seconds with up to nine fraction digits, in ns; `None` on overflow.
fn parse_delay_ns(text: &str) -> Option<u64> {
    let (whole, fraction) = match text.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (text, None),
    };
    let mut ns = parse_decimal(whole)?.checked_mul(NS_PER_SEC)?;
    if let Some(fraction) = fraction {
        if fraction.len() > MAX_DELAY_FRACTION_DIGITS {
            return None;
        }
        let mut scaled = parse_decimal(fraction)?;
        for _ in fraction.len()..MAX_DELAY_FRACTION_DIGITS {
            scaled *= 10;
        }
        ns = ns.checked_add(scaled)?;
    }
    Some(ns)
}

/// `ID:GEN`; generation 0 is never a Cell (it marks ownerless kernel tasks).
fn parse_cell(text: &str) -> Option<View> {
    let (cell_id, generation) = text.split_once(':')?;
    let generation = parse_decimal(generation).filter(|&generation| generation != 0)?;
    Some(View::Cell { cell_id: parse_decimal(cell_id)?, generation })
}

fn parse_field_list(list: &str, kind: RowKind) -> Result<Vec<Field>, &'static str> {
    let mut fields = Vec::new();
    for token in list.split(',') {
        let field = Field::resolve(token, kind)?;
        if fields.contains(&field) {
            return Err(USAGE);
        }
        fields.push(field);
    }
    Ok(fields)
}
