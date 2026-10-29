//! Turn validated telemetry snapshots into per-frame samples.
//!
//! The [`Sampler`] keeps one baseline per row, `(generation, cell_id, cpu_ns)`
//! for Cells and `((generation, tid), cpu_ns)` for tasks, in the provider's
//! order, and matches the next snapshot against it with one linear merge-join.
//! No snapshot bytes and no history are kept.

use super::text::TopError;
use super::{Field, RowKind, SortSpec, View, MAX_FRAME_ROWS};
use alloc::string::String;
use alloc::vec::Vec;
use api::telemetry::{
    cell_flags, cell_state, header_flags, name_bytes, task_flags, task_state, TelemetryCellV1,
    TelemetrySnapshot, TelemetryTaskV1,
};
use core::cmp::Ordering;

/// How a row's `cpu_tenths` was (or was not) derived.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality {
    /// No baseline for this identity (first sample, new incarnation, new epoch).
    New,
    /// Measured over the interval since the baseline.
    Ok,
    /// `cpu_ns` decreased against the baseline.
    Reset,
    /// The provider flagged CPU unavailable, the interval is not positive, or
    /// the rate does not fit `u32`.
    Unavailable,
}

impl Quality {
    /// The `cpu_quality` record value.
    pub fn label(self) -> &'static str {
        match self {
            Quality::New => "new",
            Quality::Ok => "ok",
            Quality::Reset => "reset",
            Quality::Unavailable => "unavailable",
        }
    }
}

/// CPU of one row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cpu {
    /// Cumulative on-CPU ns; `None` when the provider flagged CPU unavailable.
    pub ns: Option<u64>,
    /// Core-equivalent tenths of a percent, unclamped; `Some` only for [`Quality::Ok`].
    pub tenths: Option<u32>,
    pub quality: Quality,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellState {
    Live,
    Retiring,
}

impl CellState {
    pub fn label(self) -> &'static str {
        match self {
            CellState::Live => "Live",
            CellState::Retiring => "Retiring",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TaskState {
    Ready,
    Running,
    Waiting,
    Retiring,
}

impl TaskState {
    pub fn label(self) -> &'static str {
        match self {
            TaskState::Ready => "Ready",
            TaskState::Running => "Running",
            TaskState::Waiting => "Waiting",
            TaskState::Retiring => "Retiring",
        }
    }
}

/// One Cell incarnation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellSample {
    pub cell_id: u64,
    pub generation: u64,
    pub root_tid: u64,
    pub name: String,
    pub name_truncated: bool,
    pub state: CellState,
    pub tasks: u32,
    pub cpu: Cpu,
    /// The Cell's whole quota charge; `None` when the provider flagged it unavailable.
    pub quota_bytes: Option<u64>,
    /// The Cell's quota limit; `None` when the provider flagged it unavailable.
    pub limit_bytes: Option<u64>,
}

/// One task. Its byte counts are informational and never summed into a Cell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskSample {
    pub tid: u64,
    /// `None` for ownerless kernel tasks (provider generation 0).
    pub cell_id: Option<u64>,
    /// `None` for ownerless kernel tasks (provider generation 0).
    pub generation: Option<u64>,
    pub name: String,
    pub name_truncated: bool,
    pub state: TaskState,
    pub cpu: Cpu,
    pub stack_bytes: u64,
    pub segment_bytes: u64,
    pub stack_in_quota: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rows {
    Cells(Vec<CellSample>),
    Tasks(Vec<TaskSample>),
}

/// System memory in bytes (frames × page size).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SysMem {
    Present { total_bytes: u64, used_bytes: u64, free_bytes: u64 },
    /// Requested without the `observe_memory` right.
    Denied,
    /// Not requested, or a byte count overflowed `u64`.
    Absent,
}

/// Frame-wide producer metadata (records: Begin `meta`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameMeta {
    pub sample_id: u64,
    pub provider_epoch: u64,
    pub capture_start_ns: u64,
    /// ns since the baseline capture; `None` without a same-epoch, clocked baseline.
    pub interval_ns: Option<u64>,
    pub online_cpus: u32,
    pub cpu_available: bool,
    pub total_cells: u32,
    pub total_tasks: u32,
    /// Baseline Cells missing from this capture; `None` after an incomplete capture.
    pub gone_cells: Option<u64>,
    /// Baseline tasks missing from this capture; `None` after an incomplete
    /// capture or when either capture lacked task rows.
    pub gone_tasks: Option<u64>,
    pub mem: SysMem,
    pub memory_interval: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub meta: FrameMeta,
    pub rows: Rows,
    /// Every requested row was captured and fits [`MAX_FRAME_ROWS`].
    pub complete: bool,
    /// Rows of this frame's kind the provider or the frame bound dropped.
    pub dropped: u64,
    /// Fields unavailable in at least one row, in record order.
    pub unavailable_fields: Vec<String>,
}

impl Frame {
    /// Hide Retiring rows unless `show_all`; a [`View::Cell`] keeps only its tasks.
    pub fn retain(&mut self, show_all: bool, view: View) {
        match &mut self.rows {
            Rows::Cells(rows) => rows.retain(|row| show_all || row.state != CellState::Retiring),
            Rows::Tasks(rows) => rows.retain(|row| {
                (show_all || row.state != TaskState::Retiring) && in_view(view, row.cell_id, row.generation)
            }),
        }
    }

    /// Stable sort of this frame's rows; ties break on identity.
    pub fn sort(&mut self, spec: &SortSpec) {
        match &mut self.rows {
            Rows::Cells(rows) => rows.sort_by(|a, b| {
                compare(a.key(spec.field), b.key(spec.field), spec.descending)
                    .then((a.cell_id, a.generation).cmp(&(b.cell_id, b.generation)))
            }),
            Rows::Tasks(rows) => rows.sort_by(|a, b| {
                compare(a.key(spec.field), b.key(spec.field), spec.descending).then(a.tid.cmp(&b.tid))
            }),
        }
    }

    pub fn row_count(&self) -> usize {
        match &self.rows {
            Rows::Cells(rows) => rows.len(),
            Rows::Tasks(rows) => rows.len(),
        }
    }

    pub fn row_kind(&self) -> RowKind {
        match self.rows {
            Rows::Cells(_) => RowKind::Cells,
            Rows::Tasks(_) => RowKind::Tasks,
        }
    }

    /// `Err(TopError::Incomplete)` when rows were dropped: consumers that cannot
    /// carry truncation (`ps`) refuse the frame instead of showing a subset.
    pub fn check_complete(&self) -> Result<(), TopError> {
        if self.complete {
            return Ok(());
        }
        let total = match self.row_kind() {
            RowKind::Cells => self.meta.total_cells,
            RowKind::Tasks => self.meta.total_tasks,
        } as u64;
        let total = total.max(self.dropped);
        Err(TopError::Incomplete { emitted: total - self.dropped, total })
    }
}

/// Sort key of one field of one row; `None` when the row kind lacks the field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Key<'a> {
    Null,
    Num(u64),
    Text(&'a str),
    Flag(bool),
}

fn opt(value: Option<u64>) -> Key<'static> {
    value.map_or(Key::Null, Key::Num)
}

fn quality_rank(quality: Quality) -> u64 {
    quality as u64
}

impl CellSample {
    fn key(&self, field: Field) -> Option<Key<'_>> {
        Some(match field {
            Field::CellId => Key::Num(self.cell_id),
            Field::Generation => Key::Num(self.generation),
            Field::RootTid => Key::Num(self.root_tid),
            Field::Name => Key::Text(&self.name),
            Field::NameTruncated => Key::Flag(self.name_truncated),
            Field::State => Key::Num(self.state as u64),
            Field::Tasks => Key::Num(u64::from(self.tasks)),
            Field::CpuNs => opt(self.cpu.ns),
            Field::CpuTenths => opt(self.cpu.tenths.map(u64::from)),
            Field::CpuQuality => Key::Num(quality_rank(self.cpu.quality)),
            Field::QuotaBytes => opt(self.quota_bytes),
            Field::LimitBytes => opt(self.limit_bytes),
            Field::Tid | Field::StackBytes | Field::SegmentBytes | Field::StackInQuota => return None,
        })
    }
}

impl TaskSample {
    fn key(&self, field: Field) -> Option<Key<'_>> {
        Some(match field {
            Field::Tid => Key::Num(self.tid),
            Field::CellId => opt(self.cell_id),
            Field::Generation => opt(self.generation),
            Field::Name => Key::Text(&self.name),
            Field::NameTruncated => Key::Flag(self.name_truncated),
            Field::State => Key::Num(self.state as u64),
            Field::CpuNs => opt(self.cpu.ns),
            Field::CpuTenths => opt(self.cpu.tenths.map(u64::from)),
            Field::CpuQuality => Key::Num(quality_rank(self.cpu.quality)),
            Field::StackBytes => Key::Num(self.stack_bytes),
            Field::SegmentBytes => Key::Num(self.segment_bytes),
            Field::StackInQuota => Key::Flag(self.stack_in_quota),
            Field::RootTid | Field::Tasks | Field::QuotaBytes | Field::LimitBytes => return None,
        })
    }
}

/// Null sorts last in either direction; a field the row kind lacks sorts equal.
fn compare(a: Option<Key<'_>>, b: Option<Key<'_>>, descending: bool) -> Ordering {
    match (a, b) {
        (Some(Key::Null), Some(Key::Null)) | (None, _) | (_, None) => Ordering::Equal,
        (Some(Key::Null), Some(_)) => Ordering::Greater,
        (Some(_), Some(Key::Null)) => Ordering::Less,
        (Some(a), Some(b)) if descending => b.cmp(&a),
        (Some(a), Some(b)) => a.cmp(&b),
    }
}

fn in_view(view: View, cell_id: Option<u64>, generation: Option<u64>) -> bool {
    match view {
        View::Cell { cell_id: want_cell, generation: want_generation } => {
            cell_id == Some(want_cell) && generation == Some(want_generation)
        }
        View::Cells | View::Tasks => true,
    }
}

/// One baseline row: `(generation, cell_id | tid, cpu_ns)`. `cpu_ns` is `None`
/// when the provider flagged it unavailable, so no rate is derived from it.
#[derive(Clone, Copy, Debug)]
struct Mark {
    generation: u64,
    id: u64,
    cpu_ns: Option<u64>,
}

/// Merge-join cursor over baseline marks; current rows arrive in the same order.
struct Join<'a> {
    marks: &'a [Mark],
    at: usize,
    matched: u64,
}

impl<'a> Join<'a> {
    fn new(marks: &'a [Mark]) -> Self {
        Self { marks, at: 0, matched: 0 }
    }

    /// The baseline mark of `(generation, id)`, if any.
    fn find(&mut self, generation: u64, id: u64) -> Option<Mark> {
        let key = (generation, id);
        while let Some(mark) = self.marks.get(self.at) {
            match (mark.generation, mark.id).cmp(&key) {
                Ordering::Less => self.at += 1,
                Ordering::Equal => {
                    self.at += 1;
                    self.matched += 1;
                    return Some(*mark);
                }
                Ordering::Greater => break,
            }
        }
        None
    }

    fn gone(&self) -> u64 {
        self.marks.len() as u64 - self.matched
    }
}

/// What the previous capture leaves for the next one.
#[derive(Debug, Default)]
struct Baseline {
    present: bool,
    epoch: u64,
    /// `None` when the baseline header had no clock.
    capture_start_ns: Option<u64>,
    cells: Vec<Mark>,
    tasks: Vec<Mark>,
    /// The baseline capture held task rows.
    tasks_seen: bool,
}

/// Stateful observer of successive snapshots (one per `top` run).
#[derive(Debug, Default)]
pub struct Sampler {
    base: Baseline,
    next_cells: Vec<Mark>,
    next_tasks: Vec<Mark>,
}

/// Fields as a bitset over `Field as u32`.
#[derive(Clone, Copy, Default)]
struct FieldSet(u32);

impl FieldSet {
    fn insert(&mut self, field: Field) {
        self.0 |= 1 << field as u32;
    }

    fn contains(self, field: Field) -> bool {
        self.0 & (1 << field as u32) != 0
    }
}

struct Clock {
    header_cpu: bool,
    interval_ns: Option<u64>,
}

impl Clock {
    fn cpu(&self, row_cpu: bool, ns: u64, base: Option<Mark>) -> Cpu {
        if !(self.header_cpu && row_cpu) {
            return Cpu { ns: None, tenths: None, quality: Quality::Unavailable };
        }
        let ns_now = Some(ns);
        let Some(previous) = base.and_then(|mark| mark.cpu_ns) else {
            return Cpu { ns: ns_now, tenths: None, quality: Quality::New };
        };
        if ns < previous {
            return Cpu { ns: ns_now, tenths: None, quality: Quality::Reset };
        }
        let tenths = self
            .interval_ns
            .filter(|&interval| interval > 0)
            .and_then(|interval| u32::try_from(u128::from(ns - previous) * 1000 / u128::from(interval)).ok());
        match tenths {
            Some(tenths) => Cpu { ns: ns_now, tenths: Some(tenths), quality: Quality::Ok },
            None => Cpu { ns: ns_now, tenths: None, quality: Quality::Unavailable },
        }
    }

    fn mark_cpu(&self, row_cpu: bool, ns: u64) -> Option<u64> {
        (self.header_cpu && row_cpu).then_some(ns)
    }
}

impl Sampler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build the frame of `view` from `snap` and make `snap` the next baseline.
    ///
    /// `snap` must carry task rows when `view` shows tasks
    /// ([`super::request_flags`]); without them the task frame is empty and
    /// reports every task as dropped.
    pub fn observe(&mut self, snap: &TelemetrySnapshot<'_>, view: View) -> Frame {
        let header = *snap.header();
        let kind = view.row_kind();
        let header_cpu = header.has(header_flags::CPU_AVAILABLE);
        let captured = header.has(header_flags::COMPLETE);
        let base = &self.base;
        let same_epoch = base.present && base.epoch == header.provider_epoch;
        let interval_ns = match (same_epoch, header_cpu, base.capture_start_ns) {
            (true, true, Some(start)) => header.capture_start_ns.checked_sub(start),
            _ => None,
        };
        let clock = Clock { header_cpu, interval_ns };
        let empty: &[Mark] = &[];
        let mut unavailable = FieldSet::default();
        if !header_cpu {
            unavailable.insert(Field::CpuNs);
            unavailable.insert(Field::CpuTenths);
        }
        let mut excess = 0u64;

        let mut cell_join = Join::new(if same_epoch { &base.cells } else { empty });
        self.next_cells.clear();
        let mut cells = Vec::new();
        if kind == RowKind::Cells {
            cells.reserve_exact((header.emitted_cells as usize).min(MAX_FRAME_ROWS));
        }
        for row in snap.cells() {
            let row_cpu = row.has(cell_flags::CPU_AVAILABLE);
            let mark = cell_join.find(row.generation, row.cell_id);
            self.next_cells.push(Mark {
                generation: row.generation,
                id: row.cell_id,
                cpu_ns: clock.mark_cpu(row_cpu, row.cpu_ns),
            });
            if kind != RowKind::Cells {
                continue;
            }
            if cells.len() == MAX_FRAME_ROWS {
                excess += 1;
                continue;
            }
            let sample = cell_sample(&row, clock.cpu(row_cpu, row.cpu_ns, mark));
            note_cpu(&mut unavailable, &sample.cpu);
            if sample.quota_bytes.is_none() {
                unavailable.insert(Field::QuotaBytes);
            }
            if sample.limit_bytes.is_none() {
                unavailable.insert(Field::LimitBytes);
            }
            cells.push(sample);
        }

        let tasks_seen = kind == RowKind::Tasks;
        let task_base = if same_epoch && base.tasks_seen { &base.tasks[..] } else { empty };
        let mut task_join = Join::new(task_base);
        self.next_tasks.clear();
        let mut tasks = Vec::new();
        if tasks_seen {
            for row in snap.tasks() {
                let row_cpu = row.has(task_flags::CPU_AVAILABLE);
                let mark = task_join.find(row.generation, row.tid);
                self.next_tasks.push(Mark {
                    generation: row.generation,
                    id: row.tid,
                    cpu_ns: clock.mark_cpu(row_cpu, row.cpu_ns),
                });
                let owner = (row.generation != 0).then_some((row.cell_id, row.generation));
                if !in_view(view, owner.map(|o| o.0), owner.map(|o| o.1)) {
                    continue;
                }
                if tasks.len() == MAX_FRAME_ROWS {
                    excess += 1;
                    continue;
                }
                let sample = task_sample(&row, owner, clock.cpu(row_cpu, row.cpu_ns, mark));
                note_cpu(&mut unavailable, &sample.cpu);
                tasks.push(sample);
            }
        }

        // A first capture has nothing to lose; a truncated one cannot tell gone from dropped.
        let gone_cells = captured.then(|| if base.present { cell_join.gone() } else { 0 });
        let gone_tasks = match (captured && tasks_seen, base.present, base.tasks_seen) {
            (false, _, _) => None,
            (true, false, _) => Some(0),
            (true, true, true) => Some(task_join.gone()),
            (true, true, false) => None,
        };
        let (rows, provider_dropped) = match kind {
            RowKind::Cells => (Rows::Cells(cells), header.total_cells - header.emitted_cells),
            RowKind::Tasks => (Rows::Tasks(tasks), header.total_tasks - header.emitted_tasks),
        };
        let meta = FrameMeta {
            sample_id: header.sample_id,
            provider_epoch: header.provider_epoch,
            capture_start_ns: header.capture_start_ns,
            interval_ns,
            online_cpus: header.online_cpus,
            cpu_available: header_cpu,
            total_cells: header.total_cells,
            total_tasks: header.total_tasks,
            gone_cells,
            gone_tasks,
            mem: system_memory(&header),
            memory_interval: header.has(header_flags::MEMORY_INTERVAL),
        };
        let unavailable_fields =
            kind.fields().iter().filter(|&&field| unavailable.contains(field)).map(|field| field.name().into()).collect();

        core::mem::swap(&mut self.base.cells, &mut self.next_cells);
        core::mem::swap(&mut self.base.tasks, &mut self.next_tasks);
        self.base.present = true;
        self.base.epoch = header.provider_epoch;
        self.base.capture_start_ns = header_cpu.then_some(header.capture_start_ns);
        self.base.tasks_seen = tasks_seen;

        Frame {
            meta,
            rows,
            complete: captured && excess == 0,
            dropped: u64::from(provider_dropped) + excess,
            unavailable_fields,
        }
    }
}

fn note_cpu(unavailable: &mut FieldSet, cpu: &Cpu) {
    if cpu.ns.is_none() {
        unavailable.insert(Field::CpuNs);
    }
    if cpu.quality == Quality::Unavailable {
        unavailable.insert(Field::CpuTenths);
    }
}

fn row_name(name: &[u8; api::telemetry::NAME_LEN]) -> String {
    String::from_utf8_lossy(name_bytes(name)).into_owned()
}

fn cell_sample(row: &TelemetryCellV1, cpu: Cpu) -> CellSample {
    CellSample {
        cell_id: row.cell_id,
        generation: row.generation,
        root_tid: row.root_tid,
        name: row_name(&row.name),
        name_truncated: row.has(cell_flags::NAME_TRUNCATED),
        state: if row.state == cell_state::RETIRING { CellState::Retiring } else { CellState::Live },
        tasks: row.task_count,
        cpu,
        quota_bytes: row.has(cell_flags::QUOTA_AVAILABLE).then_some(row.quota_charged_bytes),
        limit_bytes: row.has(cell_flags::LIMIT_AVAILABLE).then_some(row.quota_limit_bytes),
    }
}

fn task_sample(row: &TelemetryTaskV1, owner: Option<(u64, u64)>, cpu: Cpu) -> TaskSample {
    let state = match row.state {
        task_state::READY => TaskState::Ready,
        task_state::RUNNING => TaskState::Running,
        task_state::WAITING => TaskState::Waiting,
        _ => TaskState::Retiring,
    };
    TaskSample {
        tid: row.tid,
        cell_id: owner.map(|owner| owner.0),
        generation: owner.map(|owner| owner.1),
        name: row_name(&row.name),
        name_truncated: row.has(task_flags::NAME_TRUNCATED),
        state,
        cpu,
        stack_bytes: row.user_stack_bytes,
        segment_bytes: row.segment_bytes,
        stack_in_quota: row.has(task_flags::STACK_IN_QUOTA),
    }
}

fn system_memory(header: &api::telemetry::TelemetryHeaderV1) -> SysMem {
    if header.has(header_flags::SYSMEM_DENIED) {
        return SysMem::Denied;
    }
    if !header.has(header_flags::SYSMEM_PRESENT) {
        return SysMem::Absent;
    }
    let bytes = |frames: u64| frames.checked_mul(header.mem_page_size);
    match (bytes(header.mem_total_frames), bytes(header.mem_used_frames), bytes(header.mem_free_frames)) {
        (Some(total_bytes), Some(used_bytes), Some(free_bytes)) => {
            SysMem::Present { total_bytes, used_bytes, free_bytes }
        }
        _ => SysMem::Absent,
    }
}
