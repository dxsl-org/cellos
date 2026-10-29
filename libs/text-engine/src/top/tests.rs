use super::records::{frame_items, meta_record, task_rows, META_FIELDS};
use super::sample::{Cpu, Frame, Quality, Rows, Sampler, SysMem};
use super::text::{format_bytes, render_frame, TopError, MAX_WIDTH};
use super::*;
use crate::pipeline::{value_to_json, Item, Value};
use alloc::string::String;
use alloc::vec::Vec;
use api::telemetry::{
    cell_flags, cell_state, encode_name, header_flags, task_flags, task_state, TelemetryCellV1,
    TelemetryHeaderV1, TelemetrySnapshot, TelemetryTaskV1, SCOPE_SYSTEM, TELEMETRY_VERSION,
};

const EPOCH: u64 = 0x9e37_79b9_7f4a_7c15;
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
        online_cpus: 2,
        scope: SCOPE_SYSTEM,
        ..TelemetryHeaderV1::default()
    }
}

fn cell(cell_id: u64, generation: u64, cpu_ns: u64, name: &str) -> TelemetryCellV1 {
    TelemetryCellV1 {
        cell_id,
        generation,
        root_tid: cell_id.saturating_mul(100),
        cpu_ns,
        quota_charged_bytes: 64 * 1024,
        quota_limit_bytes: 4 * 1024 * 1024,
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
        user_stack_bytes: 16 * 1024,
        segment_bytes: 128 * 1024,
        state: task_state::RUNNING,
        flags: task_flags::CPU_AVAILABLE,
        name: encode_name(name).0,
    }
}

/// Encode a snapshot; totals default to the emitted counts.
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
    let snap = TelemetrySnapshot::parse(bytes).expect("valid snapshot");
    sampler.observe(&snap, view)
}

fn cells_of(frame: &Frame) -> &[super::sample::CellSample] {
    match &frame.rows {
        Rows::Cells(rows) => rows,
        Rows::Tasks(_) => panic!("expected Cell rows"),
    }
}

fn tasks_of(frame: &Frame) -> &[super::sample::TaskSample] {
    match &frame.rows {
        Rows::Tasks(rows) => rows,
        Rows::Cells(_) => panic!("expected task rows"),
    }
}

fn cpu_of_cell(frame: &Frame, generation: u64) -> Cpu {
    cells_of(frame).iter().find(|row| row.generation == generation).expect("cell row").cpu
}

fn cpu_of_task(frame: &Frame, tid: u64) -> Cpu {
    tasks_of(frame).iter().find(|row| row.tid == tid).expect("task row").cpu
}

fn get<'a>(record: &'a Value, field: &str) -> &'a Value {
    record.get_field(field).unwrap_or_else(|| panic!("missing field {field}"))
}

fn names(record: &Value) -> Vec<&str> {
    match record {
        Value::Record(fields) => fields.iter().map(|(name, _)| name.as_str()).collect(),
        _ => panic!("expected a record"),
    }
}

fn has_unavailable(frame: &Frame, field: Field) -> bool {
    frame.unavailable_fields.iter().any(|name| name == field.name())
}

fn opts(args: &[&str]) -> Result<TopOptions, &'static str> {
    parse_options(args)
}

// ─── sampler ──────────────────────────────────────────────────────────────────

#[test]
fn seeded_frame_has_exact_unclamped_tenths() {
    let mut sampler = Sampler::new();
    observe(&mut sampler, &encode(header(1, SEC), &[cell(1, 10, 0, "init")], &[]), View::Cells);
    let frame = observe(&mut sampler, &encode(header(2, 2 * SEC), &[cell(1, 10, 250 * MS, "init")], &[]), View::Cells);
    assert_eq!(frame.meta.interval_ns, Some(SEC));
    assert_eq!(cpu_of_cell(&frame, 10), Cpu { ns: Some(250 * MS), tenths: Some(250), quality: Quality::Ok });
    // floor, never rounded up
    let frame = observe(&mut sampler, &encode(header(3, 5 * SEC), &[cell(1, 10, 250 * MS + 2_999_999, "init")], &[]), View::Cells);
    assert_eq!(cpu_of_cell(&frame, 10).tenths, Some(0));
    assert_eq!(cpu_of_cell(&frame, 10).quality, Quality::Ok);
}

#[test]
fn new_rows_are_null_and_a_decrease_is_reset() {
    let mut sampler = Sampler::new();
    let first = observe(&mut sampler, &encode(header(1, SEC), &[cell(1, 10, 5 * MS, "a")], &[]), View::Cells);
    assert_eq!(cpu_of_cell(&first, 10), Cpu { ns: Some(5 * MS), tenths: None, quality: Quality::New });
    assert_eq!(first.meta.interval_ns, None);
    let record = frame_items(first).find_map(|item| match item {
        Item::Row(row) => Some(row),
        _ => None,
    });
    assert_eq!(get(record.as_ref().unwrap(), "cpu_tenths"), &Value::Null);

    let reset = observe(&mut sampler, &encode(header(2, 2 * SEC), &[cell(1, 10, MS, "a")], &[]), View::Cells);
    assert_eq!(cpu_of_cell(&reset, 10), Cpu { ns: Some(MS), tenths: None, quality: Quality::Reset });
}

#[test]
fn a_new_generation_of_the_same_cell_id_is_new_and_the_old_one_gone() {
    let mut sampler = Sampler::new();
    observe(&mut sampler, &encode(header(1, SEC), &[cell(5, 10, 100 * MS, "svc")], &[]), View::Cells);
    let frame = observe(&mut sampler, &encode(header(2, 2 * SEC), &[cell(5, 11, 300 * MS, "svc")], &[]), View::Cells);
    assert_eq!(cpu_of_cell(&frame, 11).quality, Quality::New);
    assert_eq!(cpu_of_cell(&frame, 11).tenths, None);
    assert_eq!(frame.meta.gone_cells, Some(1));
}

#[test]
fn thread_exit_keeps_cell_cpu_rising_and_counts_the_gone_task() {
    let before = encode(
        header(1, SEC),
        &[TelemetryCellV1 { task_count: 2, ..cell(3, 7, 100 * MS, "app") }],
        &[task(30, 3, 7, 50 * MS, "app"), task(31, 3, 7, 50 * MS, "app.worker")],
    );
    let after = encode(
        header(2, 2 * SEC),
        &[cell(3, 7, 400 * MS, "app")],
        &[task(30, 3, 7, 150 * MS, "app")],
    );
    let mut cells = Sampler::new();
    observe(&mut cells, &before, View::Cells);
    let frame = observe(&mut cells, &after, View::Cells);
    assert_eq!(cpu_of_cell(&frame, 7).tenths, Some(300));
    assert_eq!(frame.meta.gone_cells, Some(0));

    let mut tasks = Sampler::new();
    observe(&mut tasks, &before, View::Tasks);
    let frame = observe(&mut tasks, &after, View::Tasks);
    assert_eq!(cpu_of_task(&frame, 30).tenths, Some(100));
    assert_eq!(frame.meta.gone_tasks, Some(1));
    assert_eq!(frame.meta.gone_cells, Some(0));
}

#[test]
fn two_busy_workers_show_2000_for_the_cell_and_1000_each() {
    let before = encode(
        header(1, SEC),
        &[cell(2, 4, 0, "busy")],
        &[task(20, 2, 4, 0, "busy.a"), task(21, 2, 4, 0, "busy.b")],
    );
    let after = encode(
        header(2, 2 * SEC),
        &[cell(2, 4, 2 * SEC, "busy")],
        &[task(20, 2, 4, SEC, "busy.a"), task(21, 2, 4, SEC, "busy.b")],
    );
    let mut cells = Sampler::new();
    observe(&mut cells, &before, View::Cells);
    assert_eq!(cpu_of_cell(&observe(&mut cells, &after, View::Cells), 4).tenths, Some(2000));
    let mut tasks = Sampler::new();
    observe(&mut tasks, &before, View::Tasks);
    let frame = observe(&mut tasks, &after, View::Tasks);
    assert_eq!(cpu_of_task(&frame, 20).tenths, Some(1000));
    assert_eq!(cpu_of_task(&frame, 21).tenths, Some(1000));
}

#[test]
fn unavailable_cpu_is_null_and_listed() {
    // Header clock unavailable: every row, both CPU fields.
    let mut no_clock = header(1, 0);
    no_clock.flags &= !header_flags::CPU_AVAILABLE;
    no_clock.capture_end_ns = 0;
    let row = TelemetryCellV1 { flags: cell_flags::QUOTA_AVAILABLE | cell_flags::LIMIT_AVAILABLE, ..cell(1, 1, 0, "a") };
    let mut sampler = Sampler::new();
    observe(&mut sampler, &encode(no_clock, &[row], &[]), View::Cells);
    no_clock.sample_id = 2;
    let frame = observe(&mut sampler, &encode(no_clock, &[row], &[]), View::Cells);
    assert!(!frame.meta.cpu_available);
    assert_eq!(cpu_of_cell(&frame, 1), Cpu { ns: None, tenths: None, quality: Quality::Unavailable });
    assert!(has_unavailable(&frame, Field::CpuTenths));
    assert!(has_unavailable(&frame, Field::CpuNs));

    // One row flagged: only that row is Null.
    let flagged = TelemetryCellV1 { flags: cell_flags::QUOTA_AVAILABLE | cell_flags::LIMIT_AVAILABLE, ..cell(2, 2, 0, "b") };
    let mut sampler = Sampler::new();
    observe(&mut sampler, &encode(header(1, SEC), &[cell(1, 1, 0, "a"), flagged], &[]), View::Cells);
    let frame = observe(&mut sampler, &encode(header(2, 2 * SEC), &[cell(1, 1, 100 * MS, "a"), flagged], &[]), View::Cells);
    assert_eq!(cpu_of_cell(&frame, 1).tenths, Some(100));
    assert_eq!(cpu_of_cell(&frame, 2).quality, Quality::Unavailable);
    assert_eq!(cpu_of_cell(&frame, 2).tenths, None);
    assert!(has_unavailable(&frame, Field::CpuTenths));
}

#[test]
fn epoch_change_is_all_new_and_zero_interval_is_unavailable() {
    let mut sampler = Sampler::new();
    observe(&mut sampler, &encode(header(1, SEC), &[cell(1, 1, 0, "a")], &[]), View::Cells);
    let mut other_boot = header(2, 2 * SEC);
    other_boot.provider_epoch = EPOCH + 1;
    let frame = observe(&mut sampler, &encode(other_boot, &[cell(1, 1, 100 * MS, "a")], &[]), View::Cells);
    assert_eq!(frame.meta.interval_ns, None);
    assert!(cells_of(&frame).iter().all(|row| row.cpu.quality == Quality::New && row.cpu.tenths.is_none()));

    let mut sampler = Sampler::new();
    observe(&mut sampler, &encode(header(1, SEC), &[cell(1, 1, 0, "a")], &[]), View::Cells);
    let frame = observe(&mut sampler, &encode(header(2, SEC), &[cell(1, 1, 100 * MS, "a")], &[]), View::Cells);
    assert_eq!(frame.meta.interval_ns, Some(0));
    assert_eq!(cpu_of_cell(&frame, 1).quality, Quality::Unavailable);
    assert_eq!(cpu_of_cell(&frame, 1).tenths, None);
}

#[test]
fn rate_beyond_u32_is_unavailable_not_clamped() {
    let mut sampler = Sampler::new();
    observe(&mut sampler, &encode(header(1, SEC), &[cell(1, 1, 0, "a")], &[]), View::Cells);
    // 1 ns of interval, 5 s of CPU: tenths would exceed u32.
    let frame = observe(&mut sampler, &encode(header(2, SEC + 1), &[cell(1, 1, 5 * SEC, "a")], &[]), View::Cells);
    assert_eq!(cpu_of_cell(&frame, 1).quality, Quality::Unavailable);
}

#[test]
fn quota_comes_once_per_cell_and_task_bytes_are_never_summed() {
    let no_quota = TelemetryCellV1 { flags: cell_flags::CPU_AVAILABLE, quota_charged_bytes: 0, quota_limit_bytes: 0, ..cell(2, 2, 0, "b") };
    let mut big_task = task(10, 1, 1, 0, "a");
    big_task.user_stack_bytes = 1 << 30;
    big_task.segment_bytes = 1 << 31;
    let bytes = encode(header(1, SEC), &[cell(1, 1, 0, "a"), no_quota], &[big_task, task(11, 1, 1, 0, "a2")]);
    let frame = observe(&mut Sampler::new(), &bytes, View::Cells);
    let rows = cells_of(&frame);
    assert_eq!(rows[0].quota_bytes, Some(64 * 1024));
    assert_eq!(rows[0].limit_bytes, Some(4 * 1024 * 1024));
    assert_eq!((rows[1].quota_bytes, rows[1].limit_bytes), (None, None));
    assert!(has_unavailable(&frame, Field::QuotaBytes) && has_unavailable(&frame, Field::LimitBytes));
    let records: Vec<Value> = frame_items(frame)
        .filter_map(|item| match item {
            Item::Row(row) => Some(row),
            _ => None,
        })
        .collect();
    assert_eq!(get(&records[0], "quota_bytes"), &Value::UInt(64 * 1024));
    assert_eq!(get(&records[1], "quota_bytes"), &Value::Null);
    assert_eq!(get(&records[1], "limit_bytes"), &Value::Null);

    let frame = observe(&mut Sampler::new(), &bytes, View::Tasks);
    let task_row = tasks_of(&frame).iter().find(|row| row.tid == 10).unwrap();
    assert_eq!((task_row.stack_bytes, task_row.segment_bytes), (1 << 30, 1 << 31));
}

#[test]
fn system_memory_present_denied_absent() {
    let mut present = header(1, SEC);
    present.flags |= header_flags::SYSMEM_PRESENT;
    present.mem_total_frames = 1000;
    present.mem_used_frames = 600;
    present.mem_free_frames = 400;
    present.mem_page_size = 4096;
    let frame = observe(&mut Sampler::new(), &encode(present, &[cell(1, 1, 0, "a")], &[]), View::Cells);
    assert_eq!(
        frame.meta.mem,
        SysMem::Present { total_bytes: 1000 * 4096, used_bytes: 600 * 4096, free_bytes: 400 * 4096 }
    );
    let meta = meta_record(&frame.meta, RowKind::Cells);
    assert_eq!(get(&meta, "mem_used_bytes"), &Value::UInt(600 * 4096));

    let mut denied = header(1, SEC);
    denied.flags |= header_flags::SYSMEM_DENIED;
    let frame = observe(&mut Sampler::new(), &encode(denied, &[cell(1, 1, 0, "a")], &[]), View::Cells);
    assert_eq!(frame.meta.mem, SysMem::Denied);
    let meta = meta_record(&frame.meta, RowKind::Cells);
    assert_eq!(get(&meta, "mem_total_bytes"), &Value::Null);

    let frame = observe(&mut Sampler::new(), &encode(header(1, SEC), &[cell(1, 1, 0, "a")], &[]), View::Cells);
    assert_eq!(frame.meta.mem, SysMem::Absent);

    let mut overflow = present;
    overflow.mem_page_size = u64::MAX;
    let frame = observe(&mut Sampler::new(), &encode(overflow, &[cell(1, 1, 0, "a")], &[]), View::Cells);
    assert_eq!(frame.meta.mem, SysMem::Absent);
}

#[test]
fn truncated_capture_is_incomplete_with_exact_dropped_and_null_gone() {
    let mut sampler = Sampler::new();
    observe(&mut sampler, &encode(header(1, SEC), &[cell(1, 1, 0, "a"), cell(2, 2, 0, "b"), cell(3, 3, 0, "c")], &[]), View::Cells);
    let mut cut = header(2, 2 * SEC);
    cut.flags &= !header_flags::COMPLETE;
    cut.total_cells = 3;
    let frame = observe(&mut sampler, &encode(cut, &[cell(1, 1, 0, "a"), cell(2, 2, 0, "b")], &[]), View::Cells);
    assert!(!frame.complete);
    assert_eq!(frame.dropped, 1);
    assert_eq!(frame.meta.gone_cells, None);
    assert_eq!(frame.check_complete(), Err(TopError::Incomplete { emitted: 2, total: 3 }));

    let mut cut = header(3, 3 * SEC);
    cut.flags &= !header_flags::COMPLETE;
    cut.total_tasks = 5;
    let frame = observe(&mut sampler, &encode(cut, &[cell(1, 1, 0, "a")], &[task(1, 1, 1, 0, "a")]), View::Tasks);
    assert_eq!(frame.dropped, 4);
    assert_eq!(frame.meta.gone_tasks, None);
    let end = frame_items(frame).last();
    match end {
        Some(Item::End(end)) => assert_eq!((end.complete, end.dropped), (false, 4)),
        other => panic!("expected End, got {other:?}"),
    }
}

#[test]
fn rows_beyond_the_frame_bound_are_dropped_and_reported() {
    let tasks: Vec<TelemetryTaskV1> = (1..=MAX_FRAME_ROWS as u64 + 1).map(|tid| task(tid, 1, 1, 0, "w")).collect();
    let frame = observe(&mut Sampler::new(), &encode(header(1, SEC), &[cell(1, 1, 0, "a")], &tasks), View::Tasks);
    assert_eq!(frame.row_count(), MAX_FRAME_ROWS);
    assert!(!frame.complete);
    assert_eq!(frame.dropped, 1);
}

#[test]
fn views_filter_rows_and_ownerless_tasks_have_null_cell_fields() {
    let mut retiring = cell(2, 2, 0, "old");
    retiring.state = cell_state::RETIRING;
    let mut dying = task(21, 2, 2, 0, "old");
    dying.state = task_state::RETIRING;
    let bytes = encode(
        header(1, SEC),
        &[cell(1, 1, 0, "a"), retiring],
        &[task(1, 0, 0, 0, "kernel"), task(10, 1, 1, 0, "a"), task(11, 1, 1, 0, "a.w"), dying],
    );

    let mut frame = observe(&mut Sampler::new(), &bytes, View::Cells);
    assert_eq!(frame.row_count(), 2);
    frame.retain(false, View::Cells);
    assert_eq!(cells_of(&frame).iter().map(|row| row.generation).collect::<Vec<_>>(), [1]);
    let mut all = observe(&mut Sampler::new(), &bytes, View::Cells);
    all.retain(true, View::Cells);
    assert_eq!(all.row_count(), 2);

    let mut frame = observe(&mut Sampler::new(), &bytes, View::Tasks);
    assert_eq!(frame.row_count(), 4);
    let kernel = tasks_of(&frame).iter().find(|row| row.tid == 1).unwrap();
    assert_eq!((kernel.cell_id, kernel.generation), (None, None));
    frame.retain(false, View::Tasks);
    assert_eq!(frame.row_count(), 3);
    let kernel_record = task_rows(frame).find(|row| get(row, "tid") == &Value::UInt(1)).unwrap();
    assert_eq!(get(&kernel_record, "cell_id"), &Value::Null);
    assert_eq!(get(&kernel_record, "generation"), &Value::Null);

    let view = View::Cell { cell_id: 1, generation: 1 };
    let frame = observe(&mut Sampler::new(), &bytes, view);
    let mut tids: Vec<u64> = tasks_of(&frame).iter().map(|row| row.tid).collect();
    tids.sort_unstable();
    assert_eq!(tids, [10, 11]);
    // A drill-down never matches another incarnation of the same id.
    let frame = observe(&mut Sampler::new(), &bytes, View::Cell { cell_id: 1, generation: 9 });
    assert_eq!(frame.row_count(), 0);
}

#[test]
fn sort_puts_null_last_and_breaks_ties_on_identity() {
    let mut sampler = Sampler::new();
    let base = [cell(1, 1, 0, "a"), cell(2, 2, 0, "b"), cell(3, 3, 0, "c")];
    observe(&mut sampler, &encode(header(1, SEC), &base, &[]), View::Cells);
    // gen 4 is new (Null tenths); 1 and 3 tie at 100; 2 is busiest.
    let now = [cell(1, 1, 100 * MS, "a"), cell(2, 2, 500 * MS, "b"), cell(3, 3, 100 * MS, "c"), cell(4, 4, 0, "d")];
    let mut frame = observe(&mut sampler, &encode(header(2, 2 * SEC), &now, &[]), View::Cells);
    frame.sort(&SortSpec::default_for(View::Cells));
    let order: Vec<u64> = cells_of(&frame).iter().map(|row| row.cell_id).collect();
    assert_eq!(order, [2, 1, 3, 4]);

    let mut ascending = SortSpec::new(Field::CpuTenths);
    ascending.descending = false;
    frame.sort(&ascending);
    let order: Vec<u64> = cells_of(&frame).iter().map(|row| row.cell_id).collect();
    assert_eq!(order, [1, 3, 2, 4], "Null stays last when reversed");

    frame.sort(&SortSpec::new(Field::Name));
    let order: Vec<u64> = cells_of(&frame).iter().map(|row| row.cell_id).collect();
    assert_eq!(order, [1, 2, 3, 4]);

    let tasks = [task(1, 0, 0, 0, "k"), task(7, 1, 1, 0, "z"), task(5, 2, 2, 0, "y")];
    let mut frame = observe(&mut Sampler::new(), &encode(header(1, SEC), &base, &tasks), View::Tasks);
    frame.sort(&SortSpec::new(Field::CellId));
    let order: Vec<u64> = tasks_of(&frame).iter().map(|row| row.tid).collect();
    assert_eq!(order, [7, 5, 1], "ownerless (Null cell_id) last");
}

// ─── options ──────────────────────────────────────────────────────────────────

#[test]
fn options_defaults_and_views() {
    let defaults = opts(&[]).unwrap();
    assert_eq!(defaults.view, View::Cells);
    assert_eq!(defaults.delay_ns, DEFAULT_DELAY_NS);
    assert_eq!(defaults.sort, SortSpec::new(Field::CpuTenths));
    assert!(defaults.sort.descending);
    assert!(!defaults.batch && !defaults.show_all);
    assert_eq!((defaults.count, defaults.fields), (None, None));

    let all = opts(&["-a", "-b", "-n", "3", "--tasks"]).unwrap();
    assert!(all.batch && all.show_all);
    assert_eq!(all.count, Some(3));
    assert_eq!(all.view, View::Tasks);
    assert_eq!(opts(&["--cell", "4:9"]).unwrap().view, View::Cell { cell_id: 4, generation: 9 });
    for bad in [&["--cell", "4"][..], &["--cell", "4:0"], &["--cell", ":9"], &["--cell", "x:1"], &["--cell"]] {
        assert_eq!(opts(bad), Err(USAGE), "{bad:?}");
    }
}

#[test]
fn unknown_repeated_and_conflicting_flags_are_usage_errors() {
    for bad in [
        &["-x"][..],
        &["--tasks", "--cell", "1:1"],
        &["--cell", "1:1", "--tasks"],
        &["-n", "1", "-n", "2"],
        &["-a", "-a"],
        &["-n"],
        &["-n", "-1"],
        &["-o"],
        &["--fields"],
        &["--fields", "name,,state"],
        &["--fields", "name,name"],
    ] {
        assert!(opts(bad).is_err(), "{bad:?}");
    }
    assert_eq!(USAGE_STATUS, 2);
}

#[test]
fn delay_is_decimal_seconds_with_checked_overflow() {
    assert_eq!(opts(&["-d", "0.5"]).unwrap().delay_ns, 500_000_000);
    assert_eq!(opts(&["-d", "2"]).unwrap().delay_ns, 2 * SEC);
    assert_eq!(opts(&["-d", "1.000000001"]).unwrap().delay_ns, SEC + 1);
    assert_eq!(opts(&["-d", "0"]).unwrap().delay_ns, 0);
    assert_eq!(opts(&["-d", "18446744073"]).unwrap().delay_ns, 18_446_744_073 * SEC);
    for bad in ["18446744074", "18446744073.709551616", "1.0000000001", "1.", ".5", "-1", "+1", "1e3", ""] {
        assert!(opts(&["-d", bad]).is_err(), "{bad}");
    }
}

#[test]
fn sort_aliases_follow_the_row_kind() {
    let sort = |args: &[&str]| opts(args).map(|options| options.sort.field);
    assert_eq!(sort(&["-o", "cpu"]), Ok(Field::CpuTenths));
    assert_eq!(sort(&["-o", "pid"]), Ok(Field::CellId));
    assert_eq!(sort(&["-o", "pid", "--tasks"]), Ok(Field::Tid));
    assert_eq!(sort(&["--cell", "1:1", "-o", "pid"]), Ok(Field::Tid));
    assert_eq!(sort(&["-o", "mem"]), Ok(Field::QuotaBytes));
    assert_eq!(sort(&["-o", "heap"]), Ok(Field::QuotaBytes));
    assert_eq!(sort(&["-o", "name"]), Ok(Field::Name));
    assert_eq!(sort(&["-o", "state", "--tasks"]), Ok(Field::State));
    assert_eq!(sort(&["-o", "segment_bytes", "--tasks"]), Ok(Field::SegmentBytes));
    assert!(sort(&["-o", "mem", "--tasks"]).is_err());
    assert!(sort(&["-o", "heap", "--tasks"]).is_err());
    assert!(sort(&["-o", "tid"]).is_err(), "task field on Cell rows");
    assert!(sort(&["-o", "quota_bytes", "--tasks"]).is_err(), "Cell field on task rows");
    assert!(sort(&["-o", "rss"]).is_err());
    assert!(!SortSpec::new(Field::Name).descending);
    assert!(SortSpec::new(Field::QuotaBytes).descending);

    let fields = opts(&["--tasks", "--fields", "tid,cpu,stack_bytes"]).unwrap().fields;
    assert_eq!(fields, Some(Vec::from([Field::Tid, Field::CpuTenths, Field::StackBytes])));
    assert!(opts(&["--fields", "tid"]).is_err());
}

#[test]
fn request_flags_ask_for_tasks_only_when_shown() {
    use api::telemetry::request;
    assert_eq!(request_flags(View::Cells, false), 0);
    assert_eq!(request_flags(View::Cells, true), request::SYSTEM_MEMORY);
    assert_eq!(request_flags(View::Tasks, false), request::TASKS);
    assert_eq!(request_flags(View::Cell { cell_id: 1, generation: 1 }, true), request::ALL);
}

// ─── records ──────────────────────────────────────────────────────────────────

fn default_pair() -> (Vec<u8>, Vec<u8>) {
    let mut first = header(1, 10 * SEC);
    first.flags |= header_flags::SYSMEM_PRESENT;
    first.mem_total_frames = 4096;
    first.mem_used_frames = 1024;
    first.mem_free_frames = 3072;
    first.mem_page_size = 4096;
    let mut second = first;
    second.sample_id = 2;
    second.capture_start_ns = 11 * SEC;
    second.capture_end_ns = 11 * SEC + 1_000;
    let mut retiring = cell(4, 9, 0, "old");
    retiring.state = cell_state::RETIRING;
    let before = encode(
        first,
        &[cell(1, 1, 3 * SEC, "init"), cell(2, 5, 0, "shell"), cell(3, 8, 7 * SEC, "busy"), retiring],
        &[task(1, 0, 0, 0, "idle"), task(10, 1, 1, 3 * SEC, "init"), task(20, 2, 5, 0, "shell"), task(30, 3, 8, 7 * SEC, "busy")],
    );
    let after = encode(
        second,
        &[cell(1, 1, 3 * SEC + 125 * MS, "init"), cell(2, 5, 0, "shell"), cell(3, 8, 6 * SEC, "busy"), retiring, cell(5, 12, 9 * MS, "fresh")],
        &[
            task(1, 0, 0, 900 * MS, "idle"),
            task(10, 1, 1, 3 * SEC + 125 * MS, "init"),
            task(20, 2, 5, 0, "shell"),
            task(30, 3, 8, 6 * SEC, "busy"),
            task(50, 5, 12, 9 * MS, "fresh"),
        ],
    );
    (before, after)
}

#[test]
fn schemas_have_exact_field_sets_and_types() {
    let (before, after) = default_pair();
    let mut sampler = Sampler::new();
    observe(&mut sampler, &before, View::Cells);
    let items: Vec<Item> = frame_items(observe(&mut sampler, &after, View::Cells)).collect();
    let Item::Begin(begin) = &items[0] else { panic!("Begin first") };
    let meta = begin.meta.as_ref().unwrap();
    assert_eq!(names(meta), META_FIELDS);
    assert_eq!(get(meta, "schema"), &Value::String("top.cell.v1".into()));
    assert_eq!(get(meta, "cpu_unit"), &Value::String("core_tenths".into()));
    let cell_row = items.iter().find_map(|item| match item {
        Item::Row(row) => Some(row),
        _ => None,
    });
    let cell_row = cell_row.unwrap();
    assert_eq!(
        names(cell_row),
        ["cell_id", "generation", "root_tid", "name", "name_truncated", "state", "tasks", "cpu_ns", "cpu_tenths", "cpu_quality", "quota_bytes", "limit_bytes"]
    );
    for field in ["cell_id", "generation", "root_tid", "tasks", "cpu_ns", "quota_bytes", "limit_bytes"] {
        assert!(matches!(get(cell_row, field), Value::UInt(_)), "{field}");
    }
    for field in ["name", "state", "cpu_quality"] {
        assert!(matches!(get(cell_row, field), Value::String(_)), "{field}");
    }
    assert!(matches!(get(cell_row, "name_truncated"), Value::Bool(_)));
    assert!(matches!(get(cell_row, "cpu_tenths"), Value::UInt(_) | Value::Null));

    let mut sampler = Sampler::new();
    observe(&mut sampler, &before, View::Tasks);
    let frame = observe(&mut sampler, &after, View::Tasks);
    let items: Vec<Item> = frame_items(frame).collect();
    let Item::Begin(begin) = &items[0] else { panic!("Begin first") };
    assert_eq!(get(begin.meta.as_ref().unwrap(), "schema"), &Value::String("top.task.v1".into()));
    let task_row = items.iter().find_map(|item| match item {
        Item::Row(row) if get(row, "tid") == &Value::UInt(10) => Some(row),
        _ => None,
    });
    let task_row = task_row.unwrap();
    assert_eq!(
        names(task_row),
        ["tid", "cell_id", "generation", "name", "name_truncated", "state", "cpu_ns", "cpu_tenths", "cpu_quality", "stack_bytes", "segment_bytes", "stack_in_quota"]
    );
    for field in ["tid", "cell_id", "generation", "cpu_ns", "cpu_tenths", "stack_bytes", "segment_bytes"] {
        assert!(matches!(get(task_row, field), Value::UInt(_)), "{field}");
    }
    for field in ["name", "state", "cpu_quality"] {
        assert!(matches!(get(task_row, field), Value::String(_)), "{field}");
    }
    for field in ["name_truncated", "stack_in_quota"] {
        assert!(matches!(get(task_row, field), Value::Bool(_)), "{field}");
    }
}

#[test]
fn frame_items_are_begin_rows_end_and_lazy() {
    let (before, after) = default_pair();
    let mut sampler = Sampler::new();
    observe(&mut sampler, &before, View::Cells);
    let frame = observe(&mut sampler, &after, View::Cells);
    let rows = frame.row_count();
    let unavailable = frame.unavailable_fields.clone();
    let mut items = frame_items(frame);
    match items.next() {
        Some(Item::Begin(begin)) => {
            assert_eq!((begin.sample_id, begin.monotonic_ns), (2, 11 * SEC));
        }
        other => panic!("expected Begin, got {other:?}"),
    }
    let mut seen = 0;
    let end = loop {
        match items.next() {
            Some(Item::Row(_)) => seen += 1,
            Some(Item::End(end)) => break end,
            other => panic!("unexpected {other:?}"),
        }
    };
    assert_eq!(seen, rows);
    assert!(end.complete);
    assert_eq!(end.dropped, 0);
    assert_eq!(end.unavailable_fields, unavailable);
    assert!(items.next().is_none());
}

#[test]
fn single_capture_task_rows_are_new_with_null_rate() {
    let (before, _) = default_pair();
    let frame = observe(&mut Sampler::new(), &before, View::Tasks);
    assert_eq!(frame.check_complete(), Ok(()));
    let rows: Vec<Value> = task_rows(frame).collect();
    assert_eq!(rows.len(), 4);
    for row in &rows {
        assert_eq!(get(row, "cpu_tenths"), &Value::Null);
        assert_eq!(get(row, "cpu_quality"), &Value::String("new".into()));
        assert!(matches!(get(row, "cpu_ns"), Value::UInt(_)));
    }
    let cells = observe(&mut Sampler::new(), &before, View::Cells);
    assert_eq!(task_rows(cells).count(), 0);
}

#[test]
fn a_default_frame_encodes_in_untyped_json() {
    let (before, after) = default_pair();
    for view in [View::Cells, View::Tasks] {
        let mut sampler = Sampler::new();
        observe(&mut sampler, &before, view);
        for item in frame_items(observe(&mut sampler, &after, view)) {
            match item {
                Item::Begin(begin) => {
                    let meta = begin.meta.unwrap();
                    let epoch = get(&meta, "provider_epoch");
                    let Value::String(hex) = epoch else { panic!("provider_epoch must be a String") };
                    assert_eq!(hex.len(), 16);
                    assert!(hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
                    assert_eq!(u64::from_str_radix(hex, 16), Ok(EPOCH));
                    value_to_json(&meta, false).expect("meta encodes untyped");
                }
                Item::Row(row) => {
                    value_to_json(&row, false).expect("row encodes untyped");
                }
                Item::End(_) => {}
            }
        }
    }
}

// ─── text ─────────────────────────────────────────────────────────────────────

fn render(frame: &Frame, columns: Option<&[Field]>) -> String {
    let mut out = String::new();
    render_frame(frame, columns, &mut out);
    out
}

/// Table data lines (header line and column labels skipped), split on whitespace.
fn table_rows(text: &str) -> Vec<Vec<&str>> {
    text.lines().skip(2).map(|line| line.split_whitespace().collect()).collect()
}

fn parse_tenths(cell: &str) -> Option<u64> {
    let (whole, tenth) = cell.split_once('.')?;
    if tenth.len() != 1 {
        return None;
    }
    Some(whole.parse::<u64>().ok()? * 10 + tenth.parse::<u64>().ok()?)
}

#[test]
fn telemetry_errors_are_one_line_with_status_1() {
    for error in [TopError::Denied, TopError::OutOfMemory, TopError::Malformed, TopError::Incomplete { emitted: 3, total: 9 }] {
        let message = error.message("top");
        assert!(!message.is_empty() && !message.contains('\n'), "{error:?}");
        assert!(message.starts_with("top"));
        assert_eq!(error.status(), 1);
    }
    let message = TopError::Incomplete { emitted: 3, total: 9 }.message("ps");
    let numbers: Vec<&str> = message.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).collect();
    assert_eq!(numbers, ["3", "9"]);
}

#[test]
fn truncated_frame_shows_its_dropped_count() {
    let mut cut = header(7, SEC);
    cut.flags &= !header_flags::COMPLETE;
    cut.total_cells = 40;
    let frame = observe(&mut Sampler::new(), &encode(cut, &[cell(1, 1, 0, "a")], &[]), View::Cells);
    let text = render(&frame, None);
    let first = text.lines().next().unwrap();
    assert!(first.split_whitespace().any(|token| token == "39"), "{first}");
    assert!(first.split_whitespace().any(|token| token == "7"), "{first}");
    let complete = observe(&mut Sampler::new(), &encode(header(7, SEC), &[cell(1, 1, 0, "a")], &[]), View::Cells);
    let header_line = render(&complete, None).lines().next().unwrap().to_string();
    assert!(!header_line.split_whitespace().any(|token| token == "0"), "{header_line}");
}

#[test]
fn non_ok_cpu_never_prints_zero() {
    let mut sampler = Sampler::new();
    let first = observe(&mut sampler, &encode(header(1, SEC), &[cell(1, 1, 0, "a"), cell(2, 2, 5 * MS, "b")], &[]), View::Cells);
    // New rows
    for row in table_rows(&render(&first, None)) {
        assert!(!row.contains(&"0.0") && !row.contains(&"0"), "{row:?}");
        assert_eq!(parse_tenths(row[3]), None);
    }
    // Reset, Unavailable (zero interval) and a genuinely idle Ok row.
    let reset = observe(&mut sampler, &encode(header(2, 2 * SEC), &[cell(1, 1, 0, "a"), cell(2, 2, MS, "b")], &[]), View::Cells);
    let reset_text = render(&reset, None);
    let rows = table_rows(&reset_text);
    assert_eq!(parse_tenths(rows.iter().find(|row| row[0] == "1:1").unwrap()[3]), Some(0), "idle Ok is a number");
    assert_eq!(parse_tenths(rows.iter().find(|row| row[0] == "2:2").unwrap()[3]), None, "reset is not a number");
    let zero = observe(&mut sampler, &encode(header(3, 2 * SEC), &[cell(1, 1, MS, "a"), cell(2, 2, 2 * MS, "b")], &[]), View::Cells);
    for row in table_rows(&render(&zero, None)) {
        assert_eq!(parse_tenths(row[3]), None, "{row:?}");
    }
}

#[test]
fn every_line_fits_79_columns() {
    let long = "a-very-long-cell-name-of-32-byte";
    let mut cells = Vec::new();
    let mut tasks = Vec::new();
    for index in 1..=20u64 {
        let mut row = cell(u64::MAX - index, u64::MAX - 100 + index, u64::MAX / 2, long);
        row.quota_charged_bytes = u64::MAX;
        row.quota_limit_bytes = u64::MAX;
        row.task_count = u32::MAX;
        cells.push(row);
        tasks.push(task(u64::MAX - index * 1000, u64::MAX - index, u64::MAX - 100 + index, u64::MAX / 3, long));
    }
    tasks.sort_by_key(|row| (row.generation, row.tid));
    let mut head = header(u64::MAX, SEC);
    head.flags &= !header_flags::COMPLETE;
    head.total_cells = u32::MAX;
    head.total_tasks = u32::MAX;
    head.flags |= header_flags::SYSMEM_PRESENT;
    head.mem_total_frames = u64::MAX / 4096;
    head.mem_used_frames = u64::MAX / 4096;
    head.mem_page_size = 4096;
    let bytes = encode(head, &cells, &tasks);
    for view in [View::Cells, View::Tasks] {
        let frame = observe(&mut Sampler::new(), &bytes, view);
        let kind = view.row_kind();
        for columns in [None, Some(kind.fields())] {
            let text = render(&frame, columns);
            assert_eq!(text.lines().count(), 2 + frame.row_count());
            for line in text.lines() {
                assert!(line.chars().count() <= MAX_WIDTH, "{} > {MAX_WIDTH}: {line}", line.chars().count());
            }
        }
    }
}

#[test]
fn one_snapshot_pair_gives_identical_numbers_as_text_and_records() {
    let (before, after) = default_pair();
    for view in [View::Cells, View::Tasks] {
        let mut sampler = Sampler::new();
        observe(&mut sampler, &before, view);
        let mut frame = observe(&mut sampler, &after, view);
        frame.retain(true, view);
        frame.sort(&SortSpec::default_for(view));
        let text = render(&frame, None);
        let records: Vec<Value> = frame_items(frame.clone())
            .filter_map(|item| match item {
                Item::Row(row) => Some(row),
                _ => None,
            })
            .collect();
        let rows = table_rows(&text);
        assert_eq!(rows.len(), records.len());
        let mut qualities = Vec::new();
        for (row, record) in rows.iter().zip(&records) {
            let (identity, cpu, bytes) = match view {
                // CELL NAME TASKS CPU% QUOTA LIMIT STATE
                View::Cells => {
                    let identity = alloc::format!("{}:{}", uint(get(record, "cell_id")), uint(get(record, "generation")));
                    assert_eq!(row[2], uint(get(record, "tasks")).to_string());
                    (identity, row[3], [(row[4], "quota_bytes"), (row[5], "limit_bytes")])
                }
                // TID NAME STATE CPU% STACK SEGMENTS
                _ => (uint(get(record, "tid")).to_string(), row[3], [(row[4], "stack_bytes"), (row[5], "segment_bytes")]),
            };
            assert_eq!(row[0], identity);
            assert_eq!(Value::String(row[1].into()), *get(record, "name"));
            for (cell, field) in bytes {
                assert_eq!(cell, format_bytes(uint(get(record, field))), "{field}");
            }
            let quality = get(record, "cpu_quality").clone();
            match get(record, "cpu_tenths") {
                Value::UInt(tenths) => {
                    assert_eq!(quality, Value::String("ok".into()));
                    assert_eq!(parse_tenths(cpu), Some(*tenths), "{row:?}");
                }
                Value::Null => {
                    assert_ne!(quality, Value::String("ok".into()));
                    assert_eq!(parse_tenths(cpu), None, "{row:?}");
                }
                other => panic!("cpu_tenths {other:?}"),
            }
            qualities.push(quality);
        }
        // The pair exercises Ok, New and Reset rows.
        for label in ["ok", "new", "reset"] {
            assert!(qualities.contains(&Value::String(label.into())), "{view:?} lacks {label}");
        }
    }
}

fn uint(value: &Value) -> u64 {
    match value {
        Value::UInt(value) => *value,
        other => panic!("expected UInt, got {other:?}"),
    }
}

/// The pre-S6-4 batch layout, kept as the oracle for the shared table model: every
/// cell of every row materialised first, then fitted to [`MAX_WIDTH`] and joined.
fn legacy_render(frame: &Frame, fields: Option<&[Field]>) -> String {
    use super::text::{cell_text, columns, header_text, task_text, Column};
    fn fit(columns: &[Column], widths: &mut Vec<usize>) {
        let total = |widths: &[usize]| widths.iter().sum::<usize>() + 2 * widths.len().saturating_sub(1);
        let name = columns.iter().position(|&column| column == Column::Field(Field::Name));
        let narrowest = |widths: &[usize]| match name {
            Some(name) if name < widths.len() => total(widths) - widths[name] + widths[name].min(4),
            _ => total(widths),
        };
        while widths.len() > 1 && narrowest(widths) > MAX_WIDTH {
            widths.pop();
        }
        if let Some(name) = name.filter(|&name| name < widths.len()) {
            let over = total(widths).saturating_sub(MAX_WIDTH);
            widths[name] -= over.min(widths[name].saturating_sub(4));
        }
    }
    let clip = |text: &str, max: usize| -> String { text.chars().take(max).collect() };
    let mut out = clip(&header_text(frame), MAX_WIDTH);
    out.push('\n');
    let chosen = columns(frame.row_kind(), fields);
    let mut cells: Vec<Vec<String>> = Vec::new();
    cells.push(chosen.iter().map(|column| column.label()).collect());
    match &frame.rows {
        Rows::Cells(rows) => cells.extend(rows.iter().map(|row| chosen.iter().map(|&c| cell_text(row, c)).collect())),
        Rows::Tasks(rows) => cells.extend(rows.iter().map(|row| chosen.iter().map(|&c| task_text(row, c)).collect())),
    }
    let mut widths: Vec<usize> =
        (0..chosen.len()).map(|i| cells.iter().map(|line| line[i].chars().count()).max().unwrap_or(0)).collect();
    fit(&chosen, &mut widths);
    for line in &cells {
        let mut text = String::new();
        for (index, &width) in widths.iter().enumerate() {
            if index > 0 {
                text.push_str("  ");
            }
            let cell = clip(&line[index], width);
            let pad = width - cell.chars().count();
            if chosen[index].right_aligned() {
                text.extend(core::iter::repeat(' ').take(pad));
                text.push_str(&cell);
            } else {
                text.push_str(&cell);
                if index + 1 != widths.len() {
                    text.extend(core::iter::repeat(' ').take(pad));
                }
            }
        }
        out.push_str(&clip(text.trim_end(), MAX_WIDTH));
        out.push('\n');
    }
    out
}

/// Frames that exercise every layout path: the default pair (Ok/New/Reset, Retiring
/// shown), a truncated capture, and over-wide rows that narrow NAME and drop columns.
fn layout_frames() -> Vec<Frame> {
    let mut frames = Vec::new();
    let (before, after) = default_pair();
    for view in [View::Cells, View::Tasks] {
        let mut sampler = Sampler::new();
        observe(&mut sampler, &before, view);
        let mut frame = observe(&mut sampler, &after, view);
        frame.retain(true, view);
        frame.sort(&SortSpec::default_for(view));
        frames.push(frame);
    }
    let mut cut = header(7, SEC);
    cut.flags &= !header_flags::COMPLETE;
    cut.total_cells = 40;
    frames.push(observe(&mut Sampler::new(), &encode(cut, &[cell(1, 1, 0, "a")], &[]), View::Cells));
    let long = "a-very-long-cell-name-of-32-byte";
    let mut cells = Vec::new();
    let mut tasks = Vec::new();
    for index in 1..=5u64 {
        let mut row = cell(u64::MAX - index, u64::MAX - 100 + index, u64::MAX / 2, long);
        row.quota_charged_bytes = u64::MAX;
        row.quota_limit_bytes = u64::MAX;
        row.task_count = u32::MAX;
        cells.push(row);
        tasks.push(task(u64::MAX - index * 1000, u64::MAX - index, u64::MAX - 100 + index, u64::MAX / 3, long));
    }
    tasks.sort_by_key(|row| (row.generation, row.tid));
    let bytes = encode(header(u64::MAX, SEC), &cells, &tasks);
    frames.push(observe(&mut Sampler::new(), &bytes, View::Cells));
    frames.push(observe(&mut Sampler::new(), &bytes, View::Tasks));
    frames
}

#[test]
fn batch_text_is_byte_identical_to_the_pre_refactor_layout() {
    for frame in layout_frames() {
        let kind = frame.row_kind();
        let subset = [Field::Name, Field::CpuQuality, Field::CpuNs];
        for fields in [None, Some(kind.fields()), Some(&subset[..])] {
            assert_eq!(render(&frame, fields), legacy_render(&frame, fields), "{kind:?} {fields:?}");
        }
    }
}

#[test]
fn table_widths_fit_the_requested_width() {
    use super::text::{columns, widths};
    for frame in layout_frames() {
        let kind = frame.row_kind();
        for fields in [None, Some(kind.fields())] {
            let chosen = columns(kind, fields);
            for max in [38, 75, 76, 77, 78, 79] {
                let fitted = widths(&frame, &chosen, max);
                assert!(!fitted.is_empty() && fitted.len() <= chosen.len());
                let total = fitted.iter().sum::<usize>() + 2 * (fitted.len() - 1);
                assert!(total <= max || fitted.len() == 1, "{kind:?} {max}: {fitted:?}");
            }
        }
    }
}
