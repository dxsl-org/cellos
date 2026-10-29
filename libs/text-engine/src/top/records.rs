//! `top.cell.v1`/`top.task.v1` records and Frames items (D64-1 R1: one schema per frame).

use super::sample::{CellSample, Cpu, Frame, FrameMeta, Rows, SysMem, TaskSample};
use super::{Field, RowKind};
use crate::pipeline::{FrameBegin, FrameEnd, Item, Value};
use alloc::format;
use alloc::string::String;
use alloc::vec::{self, Vec};

/// Begin `meta` field names, in record order.
pub const META_FIELDS: [&str; 15] = [
    "schema",
    "provider_epoch",
    "interval_ns",
    "online_cpus",
    "cpu_unit",
    "cpu_available",
    "total_cells",
    "total_tasks",
    "gone_cells",
    "gone_tasks",
    "mem_state",
    "mem_total_bytes",
    "mem_used_bytes",
    "mem_free_bytes",
    "memory_interval",
];

const CPU_UNIT: &str = "core_tenths";

fn uint(value: Option<u64>) -> Value {
    value.map_or(Value::Null, Value::UInt)
}

fn text(value: &str) -> Value {
    Value::String(value.into())
}

fn cpu_value(cpu: &Cpu, field: Field) -> Value {
    match field {
        Field::CpuNs => uint(cpu.ns),
        Field::CpuTenths => uint(cpu.tenths.map(u64::from)),
        _ => text(cpu.quality.label()),
    }
}

/// The record value of `field`; moves the name out of `row`.
fn cell_value(row: &mut CellSample, field: Field) -> Value {
    match field {
        Field::CellId => Value::UInt(row.cell_id),
        Field::Generation => Value::UInt(row.generation),
        Field::RootTid => Value::UInt(row.root_tid),
        Field::Name => Value::String(core::mem::take(&mut row.name)),
        Field::NameTruncated => Value::Bool(row.name_truncated),
        Field::State => text(row.state.label()),
        Field::Tasks => Value::UInt(u64::from(row.tasks)),
        Field::CpuNs | Field::CpuTenths | Field::CpuQuality => cpu_value(&row.cpu, field),
        Field::QuotaBytes => uint(row.quota_bytes),
        Field::LimitBytes => uint(row.limit_bytes),
        Field::Tid | Field::StackBytes | Field::SegmentBytes | Field::StackInQuota => Value::Null,
    }
}

/// The record value of `field`; moves the name out of `row`.
fn task_value(row: &mut TaskSample, field: Field) -> Value {
    match field {
        Field::Tid => Value::UInt(row.tid),
        Field::CellId => uint(row.cell_id),
        Field::Generation => uint(row.generation),
        Field::Name => Value::String(core::mem::take(&mut row.name)),
        Field::NameTruncated => Value::Bool(row.name_truncated),
        Field::State => text(row.state.label()),
        Field::CpuNs | Field::CpuTenths | Field::CpuQuality => cpu_value(&row.cpu, field),
        Field::StackBytes => Value::UInt(row.stack_bytes),
        Field::SegmentBytes => Value::UInt(row.segment_bytes),
        Field::StackInQuota => Value::Bool(row.stack_in_quota),
        Field::RootTid | Field::Tasks | Field::QuotaBytes | Field::LimitBytes => Value::Null,
    }
}

/// One `top.cell.v1` record.
pub fn cell_record(mut row: CellSample) -> Value {
    Value::Record(RowKind::Cells.fields().iter().map(|&field| (field.name().into(), cell_value(&mut row, field))).collect())
}

/// One `top.task.v1` record.
pub fn task_record(mut row: TaskSample) -> Value {
    Value::Record(RowKind::Tasks.fields().iter().map(|&field| (field.name().into(), task_value(&mut row, field))).collect())
}

/// The Begin `meta` record of a frame of `kind` rows.
pub fn meta_record(meta: &FrameMeta, kind: RowKind) -> Value {
    let (mem_state, total, used, free) = match meta.mem {
        SysMem::Present { total_bytes, used_bytes, free_bytes } => {
            ("present", Some(total_bytes), Some(used_bytes), Some(free_bytes))
        }
        SysMem::Denied => ("denied", None, None, None),
        SysMem::Absent => ("absent", None, None, None),
    };
    let values = [
        text(kind.schema()),
        // DV-9: always above 2^53, so a String keeps untyped JSON exact.
        Value::String(format!("{:016x}", meta.provider_epoch)),
        uint(meta.interval_ns),
        Value::UInt(u64::from(meta.online_cpus)),
        text(CPU_UNIT),
        Value::Bool(meta.cpu_available),
        Value::UInt(u64::from(meta.total_cells)),
        Value::UInt(u64::from(meta.total_tasks)),
        uint(meta.gone_cells),
        uint(meta.gone_tasks),
        text(mem_state),
        uint(total),
        uint(used),
        uint(free),
        Value::Bool(meta.memory_interval),
    ];
    Value::Record(META_FIELDS.iter().zip(values).map(|(&name, value)| (String::from(name), value)).collect())
}

enum RowIter {
    Cells(vec::IntoIter<CellSample>),
    Tasks(vec::IntoIter<TaskSample>),
}

impl RowIter {
    fn new(rows: Rows) -> Self {
        match rows {
            Rows::Cells(rows) => RowIter::Cells(rows.into_iter()),
            Rows::Tasks(rows) => RowIter::Tasks(rows.into_iter()),
        }
    }

    fn next_value(&mut self) -> Option<Value> {
        match self {
            RowIter::Cells(rows) => rows.next().map(cell_record),
            RowIter::Tasks(rows) => rows.next().map(task_record),
        }
    }
}

/// Begin, one record per pull, End.
pub struct FrameItems {
    begin: Option<FrameBegin>,
    rows: RowIter,
    end: Option<FrameEnd>,
}

impl Iterator for FrameItems {
    type Item = Item;

    fn next(&mut self) -> Option<Item> {
        if let Some(begin) = self.begin.take() {
            return Some(Item::Begin(begin));
        }
        if let Some(row) = self.rows.next_value() {
            return Some(Item::Row(row));
        }
        self.end.take().map(Item::End)
    }
}

/// The Frames items of `frame`: rows are converted lazily, one per pull.
pub fn frame_items(frame: Frame) -> FrameItems {
    let kind = frame.row_kind();
    let begin = FrameBegin {
        sample_id: frame.meta.sample_id,
        monotonic_ns: frame.meta.capture_start_ns,
        meta: Some(meta_record(&frame.meta, kind)),
    };
    let end = FrameEnd {
        complete: frame.complete,
        unavailable_fields: frame.unavailable_fields,
        dropped: frame.dropped,
    };
    FrameItems { begin: Some(begin), rows: RowIter::new(frame.rows), end: Some(end) }
}

/// `top.task.v1` records of a task frame, one per pull (`ps --records`); a Cell
/// frame yields nothing.
pub struct TaskRows {
    rows: vec::IntoIter<TaskSample>,
}

impl Iterator for TaskRows {
    type Item = Value;

    fn next(&mut self) -> Option<Value> {
        self.rows.next().map(task_record)
    }
}

pub fn task_rows(frame: Frame) -> TaskRows {
    let rows = match frame.rows {
        Rows::Tasks(rows) => rows,
        Rows::Cells(_) => Vec::new(),
    };
    TaskRows { rows: rows.into_iter() }
}
