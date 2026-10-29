//! Pure readers for the Phase 06 S6-5 `top-load` scenarios: `top --records … | to jsonl`
//! envelope lines (`top.cell.v1`/`top.task.v1` rows, Begin `meta`, End), the fixture's
//! `TOPLOAD` acknowledgement lines, the `s2tw-probe trunc` line, and the comparisons the
//! scenarios make on them. Host-tested; nothing here touches a host or a syscall.
//!
//! The JSON reader only looks up a key of one envelope line. Every key it is used with is
//! unique within its line (`"tasks":` never matches `"total_tasks":`: the quote is part of
//! the key), and no value it reads contains an escaped quote.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// The raw value token of `"key":` in `json`: a string without its quotes, or a number,
/// `true`, `false`, `null` (or an opening brace for a nested record).
pub fn raw<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\":");
    let start = json.find(&needle)? + needle.len();
    let rest = &json[start..];
    if let Some(body) = rest.strip_prefix('"') {
        return body.find('"').map(|end| &body[..end]);
    }
    let end = rest.find([',', '}', ']']).unwrap_or(rest.len());
    Some(&rest[..end])
}

/// How many times `"key":` occurs in `json`.
pub fn key_count(json: &str, key: &str) -> usize {
    json.matches(&format!("\"{key}\":")).count()
}

pub fn uint(json: &str, key: &str) -> Option<u64> {
    raw(json, key)?.parse().ok()
}

pub fn text<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\":\"");
    json.contains(&needle).then(|| raw(json, key)).flatten()
}

pub fn boolean(json: &str, key: &str) -> Option<bool> {
    match raw(json, key)? {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// One frame of an envelope stream: the Begin line, its row lines and the End line.
#[derive(Clone, Debug, Default)]
pub struct RecFrame {
    pub begin: String,
    pub rows: Vec<String>,
    pub end: String,
}

impl RecFrame {
    pub fn total_tasks(&self) -> Option<u64> {
        uint(&self.begin, "total_tasks")
    }

    pub fn online_cpus(&self) -> Option<u64> {
        uint(&self.begin, "online_cpus")
    }

    /// `gone_cells`/`gone_tasks` of the Begin `meta` (`None` for Null or absent).
    pub fn gone(&self, key: &str) -> Option<u64> {
        uint(&self.begin, key)
    }

    pub fn complete(&self) -> Option<bool> {
        boolean(&self.end, "complete")
    }

    pub fn dropped(&self) -> Option<u64> {
        uint(&self.end, "dropped")
    }

    pub fn cells(&self) -> Vec<CellRow> {
        self.rows.iter().filter_map(|row| CellRow::parse(row)).collect()
    }

    pub fn tasks(&self) -> Vec<TaskRow> {
        self.rows.iter().filter_map(|row| TaskRow::parse(row)).collect()
    }

    /// The Cell row of incarnation `id`.
    pub fn cell(&self, id: Ident) -> Option<CellRow> {
        self.cells().into_iter().find(|row| row.ident == id)
    }
}

/// Split envelope lines into frames; every line between a Begin and its End must be a row
/// and every frame must end. Lines that are not envelopes are ignored.
pub fn frames(lines: &[String]) -> Result<Vec<RecFrame>, String> {
    let mut out = Vec::new();
    let mut open: Option<RecFrame> = None;
    for line in lines.iter().filter(|l| l.starts_with("{\"frame\":")) {
        match (raw(line, "frame"), open.as_mut()) {
            (Some("begin"), None) => open = Some(RecFrame { begin: line.clone(), ..RecFrame::default() }),
            (Some("row"), Some(frame)) => frame.rows.push(line.clone()),
            (Some("end"), Some(frame)) => {
                frame.end = line.clone();
                out.extend(open.take());
            }
            (kind, _) => return Err(format!("envelope {kind:?} out of order after {} frames", out.len())),
        }
    }
    match open {
        Some(_) => Err(format!("unterminated frame after {} frames", out.len())),
        None => Ok(out),
    }
}

/// A Cell incarnation: `cell_id:generation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ident {
    pub id: u64,
    pub generation: u64,
}

impl Ident {
    /// `ID:GEN` text.
    pub fn parse(text: &str) -> Option<Ident> {
        let (id, generation) = text.split_once(':')?;
        Some(Ident { id: id.parse().ok()?, generation: generation.parse().ok()? })
    }
}

impl core::fmt::Display for Ident {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}:{}", self.id, self.generation)
    }
}

/// The fields of a `top.cell.v1` row the scenarios compare.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellRow {
    pub ident: Ident,
    pub root_tid: u64,
    pub name: String,
    pub tasks: u64,
    pub cpu_ns: Option<u64>,
    pub tenths: Option<u64>,
    pub quality: String,
    pub quota: Option<u64>,
    pub limit: Option<u64>,
}

impl CellRow {
    pub fn parse(row: &str) -> Option<CellRow> {
        if !row.starts_with("{\"frame\":\"row\"") || key_count(row, "root_tid") != 1 {
            return None;
        }
        Some(CellRow {
            ident: Ident { id: uint(row, "cell_id")?, generation: uint(row, "generation")? },
            root_tid: uint(row, "root_tid")?,
            name: String::from(text(row, "name")?),
            tasks: uint(row, "tasks")?,
            cpu_ns: uint(row, "cpu_ns"),
            tenths: uint(row, "cpu_tenths"),
            quality: String::from(text(row, "cpu_quality")?),
            quota: uint(row, "quota_bytes"),
            limit: uint(row, "limit_bytes"),
        })
    }
}

/// The fields of a `top.task.v1` row the scenarios compare.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskRow {
    pub tid: u64,
    /// `None` for an ownerless kernel task.
    pub owner: Option<Ident>,
    pub cpu_ns: Option<u64>,
    pub tenths: Option<u64>,
    pub quality: String,
    pub stack_bytes: u64,
    pub stack_in_quota: bool,
}

impl TaskRow {
    pub fn parse(row: &str) -> Option<TaskRow> {
        if !row.starts_with("{\"frame\":\"row\"") || key_count(row, "tid") != 1 {
            return None;
        }
        let owner = match (uint(row, "cell_id"), uint(row, "generation")) {
            (Some(id), Some(generation)) => Some(Ident { id, generation }),
            _ => None,
        };
        Some(TaskRow {
            tid: uint(row, "tid")?,
            owner,
            cpu_ns: uint(row, "cpu_ns"),
            tenths: uint(row, "cpu_tenths"),
            quality: String::from(text(row, "cpu_quality")?),
            stack_bytes: uint(row, "stack_bytes")?,
            stack_in_quota: boolean(row, "stack_in_quota")?,
        })
    }
}

/// The last complete line of `transcript` that starts with `TOPLOAD ` (the fixture's
/// acknowledgement), if any.
pub fn last_ack(transcript: &str) -> Option<&str> {
    let complete = &transcript[..transcript.rfind('\n')? + 1];
    complete.lines().rev().map(|l| l.trim_end_matches('\r')).find(|l| l.starts_with("TOPLOAD "))
}

/// `key=value` of an acknowledgement line as an integer.
pub fn ack_uint(line: &str, key: &str) -> Option<u64> {
    line.split_whitespace().find_map(|word| word.strip_prefix(key)?.strip_prefix('='))?.parse().ok()
}

/// The `tids=a,b,…` list of an acknowledgement line (empty list for `tids=`).
pub fn ack_tids(line: &str) -> Option<Vec<u64>> {
    let list = line.split_whitespace().find_map(|word| word.strip_prefix("tids="))?;
    if list.is_empty() {
        return Some(Vec::new());
    }
    list.split(',').map(|tid| tid.parse().ok()).collect()
}

/// `(emitted_cells, total_cells, emitted_tasks, total_tasks, complete)` of the probe's
/// `other:cells-E/T-tasks-e/U-complete-C` result.
pub fn trunc_counts(result: &str) -> Option<(u64, u64, u64, u64, u64)> {
    let rest = result.strip_prefix("other:cells-")?;
    let (cells, rest) = rest.split_once("-tasks-")?;
    let (tasks, complete) = rest.split_once("-complete-")?;
    let pair = |text: &str| -> Option<(u64, u64)> {
        let (a, b) = text.split_once('/')?;
        Some((a.parse().ok()?, b.parse().ok()?))
    };
    let (e, t) = pair(cells)?;
    let (et, u) = pair(tasks)?;
    Some((e, t, et, u, complete.parse().ok()?))
}

/// Σ `cpu_tenths` over a Cells frame's rows that have one, and the bound
/// `online_cpus × 1000 + rows` (no CPU billed twice; one tenth of rounding per row).
pub fn cpu_sum(frame: &RecFrame) -> Option<(u64, u64)> {
    let cells = frame.cells();
    let sum = cells.iter().filter_map(|row| row.tenths).sum();
    Some((sum, frame.online_cpus()? * 1000 + cells.len() as u64))
}

/// #12: a task stream across the exit of `tid`. Returns the index of the last frame that
/// still lists it.
pub fn exit_gone_tasks(frames: &[RecFrame], tid: u64) -> Result<usize, String> {
    let listed: Vec<bool> = frames.iter().map(|f| f.tasks().iter().any(|t| t.tid == tid)).collect();
    if listed.first() != Some(&true) {
        return Err(format!("tid {tid} not in the first frame: listed={listed:?}"));
    }
    let gone = listed.iter().position(|&l| !l).ok_or_else(|| format!("tid {tid} never left: listed={listed:?}"))?;
    if listed[gone..].iter().any(|&l| l) {
        return Err(format!("tid {tid} came back: listed={listed:?}"));
    }
    let count = frames[gone].gone("gone_tasks");
    let (before, after) = (frames[gone - 1].total_tasks(), frames[gone].total_tasks());
    let exact = matches!((before, after), (Some(b), Some(a)) if a + 1 == b);
    let ok = match count {
        Some(n) if exact => n == 1,
        Some(n) => n >= 1,
        None => false,
    };
    if !ok {
        return Err(format!("frame {gone} gone_tasks={count:?} total_tasks {before:?}->{after:?}"));
    }
    if let Some(reset) = frames.iter().flat_map(|f| f.tasks()).find(|t| t.quality == "reset") {
        return Err(format!("tid {} reset", reset.tid));
    }
    Ok(gone - 1)
}

/// #13: a Cells stream across a thread exit of `cell`: its task count falls from `from` to
/// `from - 1`, its `cpu_ns` never falls, its quality stays `ok`, and after the exit its
/// `cpu_ns` is at least `floor`. Returns the `cpu_ns` series.
pub fn exit_retains_cpu(frames: &[RecFrame], cell: Ident, from: u64, floor: u64) -> Result<Vec<u64>, String> {
    let rows: Vec<CellRow> = frames.iter().map(|f| f.cell(cell)).collect::<Option<_>>().ok_or_else(|| format!("{cell} missing from a frame"))?;
    let tasks: Vec<u64> = rows.iter().map(|r| r.tasks).collect();
    let cpu: Vec<u64> = rows.iter().map(|r| r.cpu_ns).collect::<Option<_>>().ok_or_else(|| format!("{cell} cpu_ns Null"))?;
    let qualities: Vec<&str> = rows.iter().map(|r| r.quality.as_str()).collect();
    let why = || format!("tasks={tasks:?} cpu_ns={cpu:?} quality={qualities:?} floor={floor}");
    let fell = tasks.first() == Some(&from)
        && tasks.last() == Some(&(from - 1))
        && tasks.windows(2).all(|p| p[1] <= p[0]);
    let monotonic = cpu.windows(2).all(|p| p[1] >= p[0]);
    let ok = qualities.iter().all(|&q| q == "ok");
    let retained = rows.iter().zip(&cpu).filter(|(r, _)| r.tasks < from).all(|(_, &ns)| ns >= floor);
    if fell && monotonic && ok && retained {
        Ok(cpu)
    } else {
        Err(why())
    }
}

/// #15: a Cells stream across a quit and relaunch of `old` as `new`: `old` listed, then a
/// frame without it carrying `gone_cells ≥ 1`, then `new` first with quality `new`, then
/// `new` with `ok`; never both in one frame and never `reset` on either.
pub fn relaunch_sequence(frames: &[RecFrame], old: Ident, new: Ident) -> Result<String, String> {
    let rows: Vec<(Option<CellRow>, Option<CellRow>)> = frames.iter().map(|f| (f.cell(old), f.cell(new))).collect();
    let shape: Vec<String> = rows
        .iter()
        .zip(frames)
        .map(|((o, n), f)| {
            format!(
                "{}{}g{}",
                o.as_ref().map_or("-", |r| if r.quality == "reset" { "O!" } else { "O" }),
                n.as_ref().map_or(String::from("-"), |r| format!("N{}", r.quality)),
                f.gone("gone_cells").map_or(String::from("?"), |n| format!("{n}"))
            )
        })
        .collect();
    let why = || format!("old={old} new={new} frames={}", shape.join(","));
    if rows.iter().any(|(o, n)| o.is_some() && n.is_some()) {
        return Err(format!("both generations in one frame: {}", why()));
    }
    if rows.iter().flat_map(|(o, n)| [o, n]).flatten().any(|r| r.quality == "reset") {
        return Err(format!("reset: {}", why()));
    }
    let present = rows.iter().position(|(o, _)| o.is_some()).ok_or_else(why)?;
    let gone = present + rows[present..].iter().position(|(o, _)| o.is_none()).ok_or_else(why)?;
    if rows[gone..].iter().any(|(o, _)| o.is_some()) || !frames[gone].gone("gone_cells").is_some_and(|n| n >= 1) {
        return Err(why());
    }
    let first = rows.iter().position(|(_, n)| n.is_some()).ok_or_else(why)?;
    let later_ok = rows[first + 1..].iter().any(|(_, n)| n.as_ref().is_some_and(|r| r.quality == "ok"));
    let first_new = rows[first].1.as_ref().is_some_and(|r| r.quality == "new");
    if first < gone || !first_new || !later_ok {
        return Err(why());
    }
    Ok(why())
}

/// #14: `quota_bytes` once on every Cell row and on no task row; `cell`'s quota covers the
/// stacks of its in-quota tasks and stays within its limit.
pub fn quota_once(cells: &RecFrame, tasks: &RecFrame, cell: Ident) -> Result<String, String> {
    if let Some(row) = cells.rows.iter().find(|r| key_count(r, "quota_bytes") != 1) {
        return Err(format!("Cell row without exactly one quota_bytes: {row}"));
    }
    if tasks.rows.iter().any(|r| key_count(r, "quota_bytes") != 0) {
        return Err(String::from("a task row carries quota_bytes"));
    }
    let row = cells.cell(cell).ok_or_else(|| format!("{cell} not in the Cells frame"))?;
    let members = tasks.tasks();
    if members.is_empty() || members.iter().any(|t| t.owner != Some(cell)) {
        return Err(format!("task frame of {cell} has {} rows, owners not all {cell}", members.len()));
    }
    let stacks: u64 = members.iter().filter(|t| t.stack_in_quota).map(|t| t.stack_bytes).sum();
    let detail = format!("quota={:?} limit={:?} in_quota_stacks={stacks} tasks={}", row.quota, row.limit, members.len());
    match (row.quota, row.limit) {
        (Some(quota), Some(limit)) if quota >= stacks && quota <= limit => Ok(detail),
        _ => Err(detail),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use alloc::vec;

    fn begin(sample: u64, total_tasks: u64, gone_cells: Option<u64>, gone_tasks: Option<u64>) -> String {
        let null = |v: Option<u64>| v.map_or("null".to_string(), |n| n.to_string());
        format!(
            "{{\"frame\":\"begin\",\"sample_id\":{sample},\"monotonic_ns\":5,\"meta\":{{\"schema\":\"top.cell.v1\",\"provider_epoch\":\"00000000000000aa\",\"interval_ns\":1000,\"online_cpus\":1,\"cpu_unit\":\"core_tenths\",\"cpu_available\":true,\"total_cells\":9,\"total_tasks\":{total_tasks},\"gone_cells\":{},\"gone_tasks\":{},\"mem_state\":\"present\",\"mem_total_bytes\":1,\"mem_used_bytes\":1,\"mem_free_bytes\":0,\"memory_interval\":true}}}}",
            null(gone_cells),
            null(gone_tasks)
        )
    }

    fn end() -> String {
        String::from("{\"frame\":\"end\",\"complete\":true,\"unavailable_fields\":[],\"dropped\":0}")
    }

    fn cell(id: u64, generation: u64, tasks: u64, cpu_ns: u64, tenths: Option<u64>, quality: &str) -> String {
        let tenths = tenths.map_or("null".to_string(), |n| n.to_string());
        format!(
            "{{\"frame\":\"row\",\"value\":{{\"cell_id\":{id},\"generation\":{generation},\"root_tid\":{},\"name\":\"top-load\",\"name_truncated\":false,\"state\":\"live\",\"tasks\":{tasks},\"cpu_ns\":{cpu_ns},\"cpu_tenths\":{tenths},\"cpu_quality\":\"{quality}\",\"quota_bytes\":4096,\"limit_bytes\":65536}}}}",
            id * 10
        )
    }

    fn task(tid: u64, owner: Option<(u64, u64)>, cpu_ns: u64, quality: &str, stack: u64, in_quota: bool) -> String {
        let (id, generation) = owner.map_or(("null".to_string(), "null".to_string()), |(i, g)| (i.to_string(), g.to_string()));
        format!(
            "{{\"frame\":\"row\",\"value\":{{\"tid\":{tid},\"cell_id\":{id},\"generation\":{generation},\"name\":\"thread\",\"name_truncated\":false,\"state\":\"waiting\",\"cpu_ns\":{cpu_ns},\"cpu_tenths\":3,\"cpu_quality\":\"{quality}\",\"stack_bytes\":{stack},\"segment_bytes\":0,\"stack_in_quota\":{in_quota}}}}}"
        )
    }

    fn stream(frames: &[(String, Vec<String>)]) -> Vec<RecFrame> {
        let mut lines = vec![String::from("top --records | to jsonl")];
        for (b, rows) in frames {
            lines.push(b.clone());
            lines.extend(rows.iter().cloned());
            lines.push(end());
        }
        super::frames(&lines).unwrap()
    }

    #[test]
    fn keys_are_matched_whole_and_values_typed() {
        let row = task(42, Some((3, 7)), 900, "ok", 8192, true);
        assert_eq!(uint(&row, "tid"), Some(42));
        assert_eq!(text(&row, "name"), Some("thread"));
        assert_eq!(boolean(&row, "stack_in_quota"), Some(true));
        assert_eq!(text(&row, "tid"), None);
        let b = begin(4, 70, Some(1), None);
        assert_eq!(uint(&b, "total_tasks"), Some(70));
        assert_eq!(uint(&b, "tasks"), None);
        assert_eq!(uint(&b, "gone_tasks"), None);
        assert_eq!(raw(&b, "gone_tasks"), Some("null"));
        assert_eq!(key_count(&cell(1, 2, 3, 4, None, "new"), "quota_bytes"), 1);
    }

    #[test]
    fn rows_parse_by_schema_and_ownerless_tasks_have_no_owner() {
        let c = CellRow::parse(&cell(3, 7, 32, 500, Some(12), "ok")).unwrap();
        assert_eq!((c.ident, c.root_tid, c.tasks, c.tenths, c.quality.as_str()), (Ident { id: 3, generation: 7 }, 30, 32, Some(12), "ok"));
        assert!(TaskRow::parse(&cell(3, 7, 32, 500, Some(12), "ok")).is_none());
        let t = TaskRow::parse(&task(9, None, 1, "new", 0, false)).unwrap();
        assert_eq!(t.owner, None);
        assert!(CellRow::parse(&task(9, None, 1, "new", 0, false)).is_none());
        assert_eq!(Ident::parse("3:7"), Some(Ident { id: 3, generation: 7 }));
        assert_eq!(Ident::parse("3"), None);
        assert_eq!(format!("{}", Ident { id: 3, generation: 7 }), "3:7");
    }

    #[test]
    fn envelopes_must_nest_and_end() {
        let lines: Vec<String> = [begin(1, 3, None, None), cell(1, 1, 1, 1, None, "new"), end(), begin(2, 3, Some(0), None), end()]
            .into_iter()
            .collect();
        let f = frames(&lines).unwrap();
        assert_eq!((f.len(), f[0].rows.len(), f[1].rows.len()), (2, 1, 0));
        assert_eq!((f[0].complete(), f[0].dropped()), (Some(true), Some(0)));
        assert!(frames(&lines[..2]).is_err());
        assert!(frames(&[cell(1, 1, 1, 1, None, "new")]).is_err());
        assert!(frames(&[begin(1, 1, None, None), begin(2, 1, None, None)]).is_err());
    }

    #[test]
    fn acknowledgements_are_whole_lines() {
        let transcript = "/data/bin/top-load 3\nTOPLOAD ready workers=3 tids=11,12,13\nTOPLOAD bu";
        let line = last_ack(transcript).unwrap();
        assert_eq!(ack_tids(line), Some(vec![11, 12, 13]));
        assert_eq!(ack_uint(line, "workers"), Some(3));
        assert_eq!(last_ack("TOPLOAD pong live=2"), None);
        assert_eq!(ack_uint("TOPLOAD pong live=2", "live"), Some(2));
        assert_eq!(ack_tids("TOPLOAD exited tids="), Some(vec![]));
        assert_eq!(ack_tids("TOPLOAD busy tids=4,x"), None);
    }

    #[test]
    fn probe_trunc_counts_parse() {
        assert_eq!(trunc_counts("other:cells-1/14-tasks-0/97-complete-0"), Some((1, 14, 0, 97, 0)));
        assert_eq!(trunc_counts("denied"), None);
        assert_eq!(trunc_counts("other:cells-1-tasks-0"), None);
    }

    #[test]
    fn cpu_sum_counts_only_rated_rows() {
        let f = &stream(&[(begin(1, 3, Some(0), None), vec![cell(1, 1, 1, 1, Some(700), "ok"), cell(2, 1, 1, 1, None, "new"), cell(3, 1, 1, 1, Some(299), "ok")])])[0];
        assert_eq!(cpu_sum(f), Some((999, 1003)));
    }

    #[test]
    fn exit_must_leave_once_with_a_gone_task() {
        let owner = Some((5, 9));
        let frame = |sample, total, gone, tids: &[u64]| (begin(sample, total, Some(0), gone), tids.iter().map(|&t| task(t, owner, 1, "ok", 0, false)).collect::<Vec<_>>());
        let good = stream(&[frame(1, 70, Some(0), &[7, 8]), frame(2, 70, Some(0), &[7, 8]), frame(3, 69, Some(1), &[8]), frame(4, 69, Some(0), &[8])]);
        assert_eq!(exit_gone_tasks(&good, 7), Ok(1));
        // Total fell by one but two tasks were counted gone.
        let wrong = stream(&[frame(1, 70, Some(0), &[7, 8]), frame(2, 69, Some(2), &[8])]);
        assert!(exit_gone_tasks(&wrong, 7).is_err());
        // Another task appeared meanwhile: at least one gone is enough.
        let busy = stream(&[frame(1, 70, Some(0), &[7, 8]), frame(2, 70, Some(1), &[8])]);
        assert_eq!(exit_gone_tasks(&busy, 7), Ok(0));
        let back = stream(&[frame(1, 70, Some(0), &[7]), frame(2, 69, Some(1), &[]), frame(3, 70, Some(0), &[7])]);
        assert!(exit_gone_tasks(&back, 7).is_err());
        let late = stream(&[frame(1, 70, Some(0), &[8]), frame(2, 69, Some(1), &[8])]);
        assert!(exit_gone_tasks(&late, 7).is_err());
        let null = stream(&[frame(1, 70, Some(0), &[7]), frame(2, 69, None, &[])]);
        assert!(exit_gone_tasks(&null, 7).is_err());
    }

    #[test]
    fn exited_thread_cpu_stays_with_its_cell() {
        let id = Ident { id: 5, generation: 9 };
        let frame = |tasks, ns, quality: &str| (begin(1, 70, Some(0), None), vec![cell(5, 9, tasks, ns, Some(1), quality)]);
        let good = stream(&[frame(31, 100, "ok"), frame(31, 150, "ok"), frame(30, 160, "ok"), frame(30, 161, "ok")]);
        assert_eq!(exit_retains_cpu(&good, id, 31, 140), Ok(vec![100, 150, 160, 161]));
        // Without retention the aggregate drops by the exited thread's CPU.
        let dropped = stream(&[frame(31, 100, "ok"), frame(30, 60, "reset")]);
        assert!(exit_retains_cpu(&dropped, id, 31, 90).is_err());
        let below = stream(&[frame(31, 100, "ok"), frame(30, 120, "ok")]);
        assert!(exit_retains_cpu(&below, id, 31, 130).is_err());
        let no_exit = stream(&[frame(31, 100, "ok"), frame(31, 120, "ok")]);
        assert!(exit_retains_cpu(&no_exit, id, 31, 0).is_err());
    }

    #[test]
    fn relaunch_needs_gone_then_new_then_ok() {
        let (old, new) = (Ident { id: 4, generation: 20 }, Ident { id: 4, generation: 31 });
        let o = |q: &str| cell(4, 20, 32, 9, Some(1), q);
        let n = |q: &str| cell(4, 31, 5, 1, None, q);
        let f = |gone, rows: Vec<String>| (begin(1, 70, Some(gone), None), rows);
        let good = stream(&[f(0, vec![o("ok")]), f(1, vec![]), f(0, vec![n("new")]), f(0, vec![n("ok")])]);
        assert!(relaunch_sequence(&good, old, new).is_ok());
        let direct = stream(&[f(0, vec![o("ok")]), f(1, vec![n("new")]), f(0, vec![n("ok")])]);
        assert!(relaunch_sequence(&direct, old, new).is_ok());
        let both = stream(&[f(0, vec![o("ok"), n("new")]), f(0, vec![n("ok")])]);
        assert!(relaunch_sequence(&both, old, new).is_err());
        let no_gone = stream(&[f(0, vec![o("ok")]), f(0, vec![]), f(0, vec![n("new")]), f(0, vec![n("ok")])]);
        assert!(relaunch_sequence(&no_gone, old, new).is_err());
        let never_ok = stream(&[f(0, vec![o("ok")]), f(1, vec![n("new")])]);
        assert!(relaunch_sequence(&never_ok, old, new).is_err());
        let reset = stream(&[f(0, vec![o("reset")]), f(1, vec![n("new")]), f(0, vec![n("ok")])]);
        assert!(relaunch_sequence(&reset, old, new).is_err());
    }

    #[test]
    fn quota_is_once_per_cell_and_covers_in_quota_stacks() {
        let id = Ident { id: 5, generation: 9 };
        let cells = &stream(&[(begin(1, 3, Some(0), None), vec![cell(5, 9, 2, 1, None, "new"), cell(6, 1, 1, 1, None, "new")])])[0];
        let tasks = &stream(&[(begin(1, 3, Some(0), Some(0)), vec![task(1, Some((5, 9)), 1, "new", 2048, true), task(2, Some((5, 9)), 1, "new", 2048, true), task(3, Some((5, 9)), 1, "new", 9999, false)])])[0];
        assert!(quota_once(cells, tasks, id).is_ok());
        let fat = &stream(&[(begin(1, 3, Some(0), Some(0)), vec![task(1, Some((5, 9)), 1, "new", 8192, true)])])[0];
        assert!(quota_once(cells, fat, id).is_err());
        let foreign = &stream(&[(begin(1, 3, Some(0), Some(0)), vec![task(1, Some((6, 1)), 1, "new", 0, true)])])[0];
        assert!(quota_once(cells, foreign, id).is_err());
    }
}
