//! Phase 06 S6-5 (`phase-06-s5-proposal.md` §4): `top` against a real multi-thread Cell
//! under known load. The fixture `/data/bin/top-load` (EMPTY sandbox launch, DV-13) runs in
//! the foreground of its own host shell (L1, L2); `top` runs in another (O); `&` is never
//! used (D65-4 H1). The fixture is steered by typed lines and answers each with one
//! `TOPLOAD` line once the effect holds (D65-5 C1); every check keys on those lines and on
//! data (`tids=`, `top --records` rows, the grid model), never on wall-clock guesses.
//! Every compared value is printed as `P04 INFO top-load <marker> key=value …` (D65-9).
//!
//! Thresholds (D65-6, TCG with one vCPU): idle ≤ 20 tenths; one spinner: Cell ≥ 600, the
//! spinner ≥ 80 % of its Cell's task sum, the parent shell < 500; two spinners: each ≥ 250
//! and within 2× of each other; Σ Cells ≤ `online_cpus` × 1000 + Cells, ≥ 800 while busy.

use super::*;
use crate::records::{
    ack_tids, ack_uint, cpu_sum, exit_gone_tasks, exit_retains_cpu, frames, last_ack, quota_once, relaunch_sequence,
    trunc_counts, CellRow, Ident, RecFrame, TaskRow,
};
use alloc::collections::BTreeSet;
use alloc::string::ToString;

/// The S6-5 markers, in proposal order (#1–#20).
const MARKERS: [&str; 20] = [
    "top-load-launch", "top-load-over-64-records", "top-load-over-64-text", "top-load-over-64-tui",
    "top-load-cpu-idle", "top-load-cpu-busy", "top-load-cpu-two-busy", "top-load-cpu-sum-bounded",
    "top-load-drill-live", "top-load-views-agree", "top-load-no-lifecycle", "top-load-exit-gone-tasks",
    "top-load-exit-retains-cpu", "top-load-quota-once", "top-load-relaunch", "top-load-id-reused",
    "top-load-gone-cell-empty", "top-load-tui-gone-notice", "top-load-teardown", "control-telemetry-truncated",
];
const FIXTURE: &str = "/data/bin/top-load";
const WORKERS: usize = 31;
/// Workers of a relaunched incarnation (#15, #18).
const RELAUNCH_WORKERS: usize = 4;
const IDLE_MAX: u64 = 20;
const BUSY_CELL_MIN: u64 = 600;
const SHELL_MAX: u64 = 500;
const TWO_BUSY_MIN: u64 = 250;
const BUSY_SUM_MIN: u64 = 800;
/// Longest wait for one fixture acknowledgement (its own settle limit is 30 s).
const ASK_MS: u64 = 90_000;
/// Longest wait for a `top --records` run on the observer.
const CAPTURE_MS: u64 = 120_000;
const BENCH_CALLS: u64 = 10_000;

fn info(marker: &str, values: &str) {
    println(&format!("P04 INFO top-load {marker} {values}"));
}

/// Emits each marker exactly once; [`Marks::rest`] fails the ones never reached.
struct Marks {
    done: Vec<&'static str>,
}

impl Marks {
    fn emit(&mut self, name: &'static str, ok: bool, why: &str) {
        if !self.done.contains(&name) {
            self.done.push(name);
            check(name, ok, why);
        }
    }

    /// `Ok(detail)` passes (the detail is logged as INFO), `Err(why)` fails.
    fn result(&mut self, name: &'static str, result: Result<String, String>) {
        match result {
            Ok(detail) => {
                info(name, &detail);
                self.emit(name, true, "");
            }
            Err(why) => {
                info(name, &format!("failed {why}"));
                self.emit(name, false, &why);
            }
        }
    }

    fn rest(&mut self, why: &str) {
        for name in MARKERS {
            self.emit(name, false, &format!("not reached: {why}"));
        }
    }
}

fn pump_all(beside: &mut [&mut Term]) {
    for term in beside.iter_mut() {
        term.pump();
    }
}

/// Pump `term` and `beside` until `done(term)`; `Err` after `ms`.
fn wait_on(term: &mut Term, beside: &mut [&mut Term], ms: u64, what: &str, mut done: impl FnMut(&Term) -> bool) -> Result<(), String> {
    let end = now_ms() + ms;
    loop {
        term.pump();
        pump_all(beside);
        if done(term) {
            return Ok(());
        }
        if now_ms() >= end {
            let tail = strip_ansi(&term.out);
            let tail: String = tail.chars().rev().take(80).collect::<Vec<_>>().into_iter().rev().collect();
            return Err(format!("timeout waiting for {what} tail={tail:?}"));
        }
        let _ = sys_set_timer(1);
    }
}

/// `Term::run` on `term` while `beside` keeps being drained.
fn run_on(term: &mut Term, beside: &mut [&mut Term], cmd: &str, ms: u64) -> Result<Vec<String>, String> {
    term.send(cmd)?;
    wait_on(term, beside, ms, cmd, Term::at_prompt)?;
    Ok(term.result(cmd))
}

/// `$?` of the last command on `term` as a number.
fn status_on(term: &mut Term, beside: &mut [&mut Term], var: &str) -> Result<i64, String> {
    let lines = run_on(term, beside, &format!("echo {var}=$?"), 30_000)?;
    match lines.as_slice() {
        [line] => line.strip_prefix(&format!("{var}=")).and_then(|v| v.parse().ok()).ok_or_else(|| format!("status line {line:?}")),
        other => Err(format!("status lines {other:?}")),
    }
}

/// Type `line` on `l` and wait for the one `TOPLOAD` acknowledgement it causes; it must
/// start with `want`. A prompt instead (the fixture or the launch ended) is an error.
fn ask(l: &mut Term, beside: &mut [&mut Term], line: &str, want: &str) -> Result<String, String> {
    l.send(line)?;
    wait_on(l, beside, ASK_MS, line, |t| t.at_prompt() || last_ack(&strip_ansi(&t.out)).is_some())?;
    let text = strip_ansi(&l.out);
    match last_ack(&text) {
        Some(ack) if ack.starts_with(want) => Ok(String::from(ack)),
        other => Err(format!("{line}: acknowledgement {other:?} prompt={}", l.at_prompt())),
    }
}

/// Launch the fixture with `workers` workers on `l`: its worker TIDs.
fn launch(l: &mut Term, beside: &mut [&mut Term], workers: usize) -> Result<Vec<u64>, String> {
    let ready = ask(l, beside, &format!("{FIXTURE} {workers}"), "TOPLOAD ready")?;
    match (ack_uint(&ready, "workers"), ack_tids(&ready)) {
        (Some(n), Some(tids)) if n as usize == workers && tids.len() == workers => Ok(tids),
        _ => Err(format!("ready line {ready:?}")),
    }
}

/// `quit` the fixture on `l`: `bye`, the prompt, and its exit status.
fn quit(l: &mut Term, beside: &mut [&mut Term], var: &str) -> Result<i64, String> {
    ask(l, beside, "quit", "TOPLOAD bye")?;
    wait_on(l, beside, 60_000, "prompt after quit", Term::at_prompt)?;
    status_on(l, beside, var)
}

/// Poll `top -b -n 1 -a` on `l` until no row is incarnation `gone` (retirement finished,
/// the slot is reusable).
fn wait_retired(l: &mut Term, beside: &mut [&mut Term], gone: Ident) -> Result<u32, String> {
    let token = gone.to_string();
    for poll in 1..=30u32 {
        let lines = run_on(l, beside, "top -b -n 1 -a", 60_000)?;
        if !lines.iter().any(|line| line.split_whitespace().next() == Some(token.as_str())) {
            return Ok(poll);
        }
    }
    Err(format!("{gone} still listed after 30 polls"))
}

/// The frames of one finite `top --records … | to jsonl` run on `o`.
fn capture(o: &mut Term, cmd: &str) -> Result<Vec<RecFrame>, String> {
    let lines = o.run(cmd, CAPTURE_MS)?;
    let found = frames(&lines)?;
    if found.is_empty() {
        return Err(format!("no frame from {cmd}"));
    }
    Ok(found)
}

/// Start the stream `cmd` on `o`, wait for its first End, run `act` (which must keep `o`
/// drained), then wait for the stream to finish: its frames and `act`'s value.
fn stream_across<T>(o: &mut Term, cmd: &str, act: impl FnOnce(&mut Term) -> Result<T, String>) -> Result<(Vec<RecFrame>, T), String> {
    o.send(cmd)?;
    wait_on(o, &mut [], CAPTURE_MS, "first frame", |t| find(&t.out, b"{\"frame\":\"end\"").is_some())?;
    let acted = act(o);
    wait_on(o, &mut [], 300_000, "end of stream", Term::at_prompt)?;
    let found = frames(&o.result(cmd))?;
    Ok((found, acted?))
}

/// The one incarnation owning every TID of `tids` in `tasks`.
fn owner_of(tasks: &[TaskRow], tids: &[u64]) -> Option<Ident> {
    let mut owners = tids.iter().map(|tid| tasks.iter().find(|t| t.tid == *tid).and_then(|t| t.owner));
    let first = owners.next()??;
    owners.all(|owner| owner == Some(first)).then_some(first)
}

fn tenths(frame: &RecFrame, id: Ident) -> Option<u64> {
    frame.cell(id).and_then(|row| row.tenths)
}

fn sorted(mut tids: Vec<u64>) -> Vec<u64> {
    tids.sort_unstable();
    tids
}

/// The system TID set of one `top --records --tasks -a` frame read on `a`.
fn system_tids(a: &mut Term) -> Result<Vec<u64>, String> {
    let found = capture(a, "top --records --tasks -a --frames 1 | to jsonl")?;
    Ok(sorted(found[0].tasks().iter().map(|t| t.tid).collect()))
}

/// The loaded scenario state.
struct Load {
    l1: Ident,
    l2: Ident,
    l1_root: u64,
    /// The fixture's worker TIDs on L1 (from `ready`).
    l1_tids: Vec<u64>,
    /// Cells frames for #8, each with whether a spinner was busy across its interval.
    cells_frames: Vec<(RecFrame, bool)>,
}

impl Load {
    /// L1's expected task set: its workers and its root.
    fn l1_members(&self) -> Vec<u64> {
        let mut all = self.l1_tids.clone();
        all.push(self.l1_root);
        sorted(all)
    }

    fn keep(&mut self, found: &[RecFrame], busy: bool) {
        self.cells_frames.extend(found.iter().cloned().map(|f| (f, busy)));
    }
}

/// Phase 06 S6-5 entry: the truncated provider capture on `a`, then the loaded scenarios
/// on three fresh hosts, which are closed again before the later host-count checks.
pub fn top_load(a: &mut Term) {
    let mut marks = Marks { done: Vec::new() };
    control_truncated(a, &mut marks);
    let baseline = count_cellbox(a);
    let hosts = (open_shell(30, 100), open_shell(24, 80), open_shell(24, 80));
    match hosts {
        (Ok(mut o), Ok(mut l1), Ok(mut l2)) => {
            if let Err(why) = scenarios(a, &mut o, &mut l1, &mut l2, &mut marks) {
                marks.rest(&why);
            }
            for term in [&mut o, &mut l1, &mut l2] {
                let _ = term.host.close();
            }
        }
        (o, l1, l2) => {
            let why = format!("hosts o={:?} l1={:?} l2={:?}", o.as_ref().err(), l1.as_ref().err(), l2.as_ref().err());
            for term in [o, l1, l2].into_iter().flatten() {
                let mut term = term;
                let _ = term.host.close();
            }
            marks.rest(&why);
        }
    }
    marks.rest("scenario ended early");
    let end = now_ms() + 30_000;
    while now_ms() < end && count_cellbox(a).ok() != baseline.as_ref().ok().copied() {
        sleep_ms(1000);
    }
}

/// #20 (D65-8 T-a): `/bin/s2tw-probe trunc` (X right) captures with tasks requested into a
/// header + one Cell row buffer: exactly one Cell emitted of more, no task, not complete.
fn control_truncated(a: &mut Term, marks: &mut Marks) {
    let result = (|| -> Result<String, String> {
        let lines = a.run("/bin/s2tw-probe trunc", 60_000)?;
        let st = status_on(a, &mut [], "sttrunc")?;
        let probe: Vec<&String> = lines.iter().filter(|l| l.starts_with("S2TW ")).collect();
        let result = match probe.as_slice() {
            [line] => line.strip_prefix("S2TW telemetry-trunc ").ok_or_else(|| format!("line {line:?}"))?,
            other => return Err(format!("probe lines {other:?}")),
        };
        let counts = trunc_counts(result).ok_or_else(|| format!("result {result:?}"))?;
        let (emitted, total, tasks, total_tasks, complete) = counts;
        let detail = format!("emitted_cells={emitted} total_cells={total} emitted_tasks={tasks} total_tasks={total_tasks} complete={complete} status={st}");
        if emitted == 1 && emitted < total && tasks == 0 && total_tasks > 0 && complete == 0 && st == 0 {
            Ok(detail)
        } else {
            Err(detail)
        }
    })();
    marks.result("control-telemetry-truncated", result);
}

fn scenarios(a: &mut Term, o: &mut Term, l1: &mut Term, l2: &mut Term, marks: &mut Marks) -> Result<(), String> {
    let l1_tids = launch(l1, &mut [], WORKERS).map_err(|why| format!("L1 launch {why}"))?;
    let l2_tids = launch(l2, &mut [], WORKERS).map_err(|why| format!("L2 launch {why}"))?;
    info("launch", &format!("l1_tids={l1_tids:?}"));
    info("launch", &format!("l2_tids={l2_tids:?}"));
    // D65-10: per-call GetTime(0) and Yield cost with nothing busy (INFO only).
    match ask(l1, &mut [], &format!("bench {BENCH_CALLS}"), "TOPLOAD bench") {
        Ok(line) => info(
            "bench",
            &format!(
                "calls={BENCH_CALLS} clock_ns={:?} yield_ns={:?} (K1 hot-path upper bound: one counter read per pass)",
                ack_uint(&line, "clock_ns"),
                ack_uint(&line, "yield_ns")
            ),
        ),
        Err(why) => info("bench", &format!("failed {why}")),
    }

    // #1: two `top-load` Cells, 32 tasks each.
    let first = capture(o, "top --records --frames 1 | to jsonl")?;
    let named: Vec<CellRow> = first[0].cells().into_iter().filter(|c| c.name == "top-load").collect();
    let idents: Vec<String> = named.iter().map(|c| format!("{}/tasks={}", c.ident, c.tasks)).collect();
    let launched = named.len() == 2 && named[0].ident != named[1].ident && named.iter().all(|c| c.tasks == WORKERS as u64 + 1);
    marks.result("top-load-launch", if launched { Ok(format!("cells={idents:?}")) } else { Err(format!("cells={idents:?}")) });

    // #2: every task in one complete records frame; `ps --records` too.
    let tasks = capture(o, "top --records --tasks -a --frames 1 | to jsonl")?;
    let frame = &tasks[0];
    let rows = frame.tasks();
    let ps = o.run("ps --records | to jsonl", CAPTURE_MS).map(|lines| rows_of(&lines).len());
    let ps_status = status_on(o, &mut [], "stpsr");
    let detail = format!(
        "total_tasks={:?} rows={} complete={:?} dropped={:?} ps_rows={ps:?} ps_status={ps_status:?}",
        frame.total_tasks(),
        rows.len(),
        frame.complete(),
        frame.dropped()
    );
    let over = frame.total_tasks().is_some_and(|n| n > 64 && n == rows.len() as u64)
        && frame.complete() == Some(true)
        && frame.dropped() == Some(0)
        && matches!(ps, Ok(n) if n > 64)
        && matches!(ps_status, Ok(0));
    marks.result("top-load-over-64-records", if over { Ok(detail) } else { Err(detail) });

    let (Some(l1_id), Some(l2_id)) = (owner_of(&rows, &l1_tids), owner_of(&rows, &l2_tids)) else {
        return Err(String::from("fixture TIDs not owned by one Cell each in the task frame"));
    };
    let l1_root = named.iter().find(|c| c.ident == l1_id).map(|c| c.root_tid).ok_or("L1 not among the top-load Cells")?;
    let mut load = Load { l1: l1_id, l2: l2_id, l1_root, l1_tids, cells_frames: Vec::new() };
    load.keep(&first, false);
    info("identities", &format!("l1={l1_id} l1_root={l1_root} l2={l2_id} l1_shell={:?} l2_shell={:?}", shell_identity(l1), shell_identity(l2)));

    over_64_text(o, marks);
    over_64_tui(a, o, marks);
    cpu_idle(o, &mut load, marks);
    cpu_busy(o, l1, &mut load, marks)?;
    cpu_two_busy(o, l1, &mut load, marks)?;
    tui_drill_views(o, l1, &load, marks)?;
    let floor = exit_gone(o, l1, &load, marks)?;
    exit_retains(o, l1, &mut load, floor, marks)?;
    quota(o, &load, marks);
    match relaunch(o, l1, &mut load, marks) {
        Ok((old, new)) => {
            gone_cell_empty(o, old, new, marks);
            tui_gone_notice(o, l1, new, marks);
        }
        Err(why) => {
            for name in ["top-load-gone-cell-empty", "top-load-tui-gone-notice"] {
                marks.emit(name, false, &format!("no relaunched incarnation: {why}"));
            }
        }
    }
    sum_bounded(&load, marks);
    teardown(o, l1, l2, marks);
    Ok(())
}

/// #3: the text forms list more than 64 tasks, completely.
fn over_64_text(o: &mut Term, marks: &mut Marks) {
    let top = o.run("top -b -n 1 --tasks", CAPTURE_MS);
    let top_status = status_on(o, &mut [], "sttb");
    let ps = o.run("ps", CAPTURE_MS);
    let ps_status = status_on(o, &mut [], "stps");
    let header_complete = top.as_ref().is_ok_and(|lines| {
        lines.iter().any(|l| l.starts_with("sample ") && l.split_whitespace().any(|w| w == "complete"))
    });
    let top_rows = top.as_ref().map(|lines| first_ints(lines).len()).unwrap_or(0);
    let ps_rows = ps.as_ref().map(|lines| first_ints(lines).len()).unwrap_or(0);
    let detail = format!("top_rows={top_rows} header_complete={header_complete} top_status={top_status:?} ps_rows={ps_rows} ps_status={ps_status:?}");
    let ok = header_complete && top_rows > 64 && ps_rows > 64 && matches!(top_status, Ok(0)) && matches!(ps_status, Ok(0));
    marks.result("top-load-over-64-text", if ok { Ok(detail) } else { Err(detail) });
}

/// Tokens of the settled frame's rows that are task rows (integer first token).
fn task_rows(s: &Screen) -> Option<Vec<Vec<String>>> {
    let rows = s.frame()?.rows();
    (!rows.is_empty() && rows.iter().all(|r| r.first().is_some_and(|t| t.parse::<u64>().is_ok()))).then_some(rows)
}

fn collect_tids(o: &Term, seen: &mut BTreeSet<u64>) {
    if let Some(rows) = task_rows(&o.screen) {
        seen.extend(rows.iter().filter_map(|r| r.first()?.parse::<u64>().ok()));
    }
}

/// After a failed TUI step: leave a `top` still drawing on `o` (`q`) so the next command
/// reaches the shell. A failure here shows in the next command's own result.
fn recover_prompt(o: &mut Term) {
    o.pump();
    if !o.at_prompt() {
        let _ = type_keys(o, "q");
        let _ = o.wait_prompt(30_000);
    }
}

/// Quit the TUI on `o` with `q`: the CANONICAL restore in order and status 0.
fn quit_tui(o: &mut Term, beside: &mut [&mut Term], var: &str) -> Result<(), String> {
    o.pump();
    o.out.clear();
    o.err.clear();
    type_keys(o, "q")?;
    wait_on(o, beside, 30_000, "prompt after q", Term::at_prompt)?;
    let (restored, at) = in_order(&o.out, &TUI_RESTORE);
    let st = status_on(o, beside, var)?;
    if restored && st == 0 {
        Ok(())
    } else {
        Err(format!("restore={at:?} status={st}"))
    }
}

/// #4: the task TUI pages through every task (> 64) of a stable population: the TIDs seen
/// equal a records capture taken just before and just after, the highlight stops on the
/// largest TID, the status line says `complete`, `q` restores, no allocation failure.
fn over_64_tui(a: &mut Term, o: &mut Term, marks: &mut Marks) {
    let result = (|| -> Result<String, String> {
        let before = system_tids(a)?;
        o.host.resize(40, 120).map_err(|s| format!("resize {s:?}"))?;
        o.send("top --tasks -a -o tid")?;
        wait_screen(o, 60_000, "task frame", |s| task_rows(s).map(|_| ()))?;
        let mut seen = BTreeSet::new();
        collect_tids(o, &mut seen);
        let mut last = o.screen.frame().and_then(|f| f.selected_id());
        let mut pages = 0;
        for _ in 0..32 {
            press(o, PAGE_DOWN)?;
            let moved = wait_screen(o, 10_000, "page down", |s| {
                s.frame().and_then(|f| f.selected_id()).filter(|id| Some(id) != last.as_ref())
            });
            match moved {
                Ok(id) => {
                    last = Some(id);
                    pages += 1;
                    collect_tids(o, &mut seen);
                }
                Err(_) => break,
            }
        }
        collect_tids(o, &mut seen);
        let status = o.screen.frame().map(|f| f.status()).unwrap_or_default();
        let complete = status.split_whitespace().any(|w| w == "complete");
        let oom = oom_text(o);
        let quit = quit_tui(o, &mut [], "sttui4");
        let after = system_tids(a)?;
        let shown: Vec<u64> = seen.into_iter().collect();
        let last_tid = last.as_deref().and_then(|t| t.parse::<u64>().ok());
        let detail = format!(
            "before={} after={} shown={} pages={pages} last={last_tid:?} max={:?} complete={complete} oom={oom} quit={quit:?}",
            before.len(),
            after.len(),
            shown.len(),
            before.last()
        );
        let ok = before.len() > 64 && before == after && shown == before && last_tid == before.last().copied() && complete && !oom && quit.is_ok();
        if ok { Ok(detail) } else { Err(format!("{detail} missing={:?}", before.iter().filter(|t| !shown.contains(t)).take(8).collect::<Vec<_>>())) }
    })();
    recover_prompt(o);
    marks.result("top-load-over-64-tui", result);
}

/// #5: both fixture Cells parked: rated `ok` and at most [`IDLE_MAX`] in every frame.
fn cpu_idle(o: &mut Term, load: &mut Load, marks: &mut Marks) {
    let result = capture(o, "top --records -d 2 --frames 2 | to jsonl").and_then(|found| {
        load.keep(&found, false);
        let values: Vec<String> = found
            .iter()
            .map(|f| format!("{:?}/{:?}", f.cell(load.l1).map(|r| (r.tenths, r.quality)), f.cell(load.l2).map(|r| (r.tenths, r.quality))))
            .collect();
        let idle = |row: Option<CellRow>| row.is_some_and(|r| r.quality == "ok" && r.tenths.is_some_and(|t| t <= IDLE_MAX));
        let ok = found.iter().all(|f| idle(f.cell(load.l1)) && idle(f.cell(load.l2)));
        let detail = format!("l1/l2={values:?} max={IDLE_MAX}");
        if ok { Ok(detail) } else { Err(detail) }
    });
    marks.result("top-load-cpu-idle", result);
}

/// Task rows of the last frame of `top --records --cell ID -d 2 --frames 2`.
fn cell_tasks(o: &mut Term, id: Ident) -> Result<Vec<TaskRow>, String> {
    let found = capture(o, &format!("top --records --cell {id} -d 2 --frames 2 | to jsonl"))?;
    Ok(found.last().map(RecFrame::tasks).unwrap_or_default())
}

/// #6: one spinner on L1.
fn cpu_busy(o: &mut Term, l1: &mut Term, load: &mut Load, marks: &mut Marks) -> Result<(), String> {
    let ack = ask(l1, &mut [], "busy 1", "TOPLOAD busy")?;
    let busy = ack_tids(&ack).filter(|t| t.len() == 1).ok_or_else(|| format!("busy ack {ack:?}"))?[0];
    let shell = shell_identity(l1).and_then(|id| Ident::parse(&id));
    let result = capture(o, "top --records -d 2 --frames 2 | to jsonl").and_then(|found| {
        load.keep(&found, true);
        let last = found.last().ok_or("no frame")?;
        let (cell, other, parent) = (tenths(last, load.l1), tenths(last, load.l2), shell.and_then(|id| tenths(last, id)));
        let rows = cell_tasks(o, load.l1)?;
        let spinner = rows.iter().find(|t| t.tid == busy).and_then(|t| t.tenths);
        let rest = rows.iter().filter(|t| t.tid != busy).filter_map(|t| t.tenths).max();
        let sum: u64 = rows.iter().filter_map(|t| t.tenths).sum();
        let detail = format!(
            "busy_tid={busy} l1_tenths={cell:?} l2_tenths={other:?} l1_shell={shell:?} l1_shell_tenths={parent:?} busy_tid_tenths={spinner:?} next_task_tenths={rest:?} l1_task_sum={sum} rows={}",
            rows.len()
        );
        let ok = cell.is_some_and(|v| v >= BUSY_CELL_MIN)
            && other.is_some_and(|v| v <= IDLE_MAX)
            && parent.is_some_and(|v| v < SHELL_MAX)
            && spinner.is_some_and(|v| rest.is_none_or(|r| v > r) && v * 10 >= sum * 8);
        if ok { Ok(detail) } else { Err(detail) }
    });
    marks.result("top-load-cpu-busy", result);
    Ok(())
}

/// #7: two spinners on L1 share it.
fn cpu_two_busy(o: &mut Term, l1: &mut Term, load: &mut Load, marks: &mut Marks) -> Result<(), String> {
    let ack = ask(l1, &mut [], "busy 2", "TOPLOAD busy")?;
    let busy = ack_tids(&ack).filter(|t| t.len() == 2).ok_or_else(|| format!("busy ack {ack:?}"))?;
    let result = capture(o, "top --records -d 2 --frames 2 | to jsonl").and_then(|found| {
        load.keep(&found, true);
        let cell = found.last().and_then(|f| tenths(f, load.l1));
        let rows = cell_tasks(o, load.l1)?;
        let rates: Vec<Option<u64>> = busy.iter().map(|tid| rows.iter().find(|t| t.tid == *tid).and_then(|t| t.tenths)).collect();
        let detail = format!("busy_tids={busy:?} busy_tenths={rates:?} l1_tenths={cell:?}");
        let ok = match rates.as_slice() {
            [Some(a), Some(b)] => *a >= TWO_BUSY_MIN && *b >= TWO_BUSY_MIN && a.max(b) <= &(2 * a.min(b)),
            _ => false,
        } && cell.is_some_and(|v| v >= BUSY_CELL_MIN);
        if ok { Ok(detail) } else { Err(detail) }
    });
    marks.result("top-load-cpu-two-busy", result);
    Ok(())
}

/// The drilled view of `id` on `o`: the frame's sample id and its rows (all task rows).
fn drill_frame(o: &mut Term, id: Ident, after: Option<u64>) -> Result<(u64, Vec<Vec<String>>), String> {
    let token = id.to_string();
    wait_screen(o, 30_000, &format!("drill frame of {token} after {after:?}"), |s| {
        let f = s.frame()?;
        let sample = f.sample()?;
        if !f.title().split_whitespace().any(|t| t == token) || after.is_some_and(|a| sample <= a) {
            return None;
        }
        Some((sample, task_rows(s)?))
    })
}

fn row_tids(rows: &[Vec<String>]) -> Vec<u64> {
    sorted(rows.iter().filter_map(|r| r.first()?.parse().ok()).collect())
}

/// Start the Cells TUI on `o`, sort by `cell_id` (four `o`: QUOTA, LIMIT, STATE, CELL) so
/// rows keep their places, and highlight `id`.
fn tui_select(o: &mut Term, beside: &mut [&mut Term], id: Ident) -> Result<(), String> {
    o.send("top")?;
    pump_all(beside);
    wait_cell_row(o, "top-load", 60_000)?;
    for _ in 0..4 {
        sort_key(o, "o")?;
    }
    select_row(o, &id.to_string())
}

/// Esc back to the Cells view with `id` listed and highlighted.
fn back_to_cells(o: &mut Term, id: Ident) -> Result<(), String> {
    let token = id.to_string();
    press(o, ESCAPE)?;
    wait_screen(o, 30_000, "Cells view after Esc", |s| {
        let f = s.frame()?;
        let listed = f.rows().iter().any(|r| r.first() == Some(&token));
        (listed && f.selected_id().as_deref() == Some(token.as_str())).then_some(())
    })
}

/// #9 (one spinner), #10 (parked: TUI = batch = records membership) and #11 (`k`, `K`,
/// Delete on L1's row change nothing) in one TUI session on `o`.
fn tui_drill_views(o: &mut Term, l1: &mut Term, load: &Load, marks: &mut Marks) -> Result<(), String> {
    let members = load.l1_members();
    let ack = ask(l1, &mut [], "busy 1", "TOPLOAD busy")?;
    let busy = ack_tids(&ack).filter(|t| t.len() == 1).ok_or_else(|| format!("busy ack {ack:?}"))?[0];
    if let Err(why) = tui_select(o, &mut [], load.l1) {
        recover_prompt(o);
        for name in ["top-load-drill-live", "top-load-views-agree", "top-load-no-lifecycle"] {
            marks.emit(name, false, &format!("TUI {why}"));
        }
        ask(l1, &mut [], "idle", "TOPLOAD idle")?;
        return Ok(());
    }
    // #9: Enter drills into L1; the first drilled frame rates every task `new`, the next
    // one numerically, with the spinner on top.
    let drill = (|| -> Result<String, String> {
        press(o, ENTER)?;
        let (sample, rows) = drill_frame(o, load.l1, None)?;
        let fresh = rows.iter().all(|r| r.get(3).map(String::as_str) == Some("new"));
        let (next, rows2) = drill_frame(o, load.l1, Some(sample))?;
        let rates: Vec<(u64, Option<u64>)> =
            rows2.iter().filter_map(|r| Some((r.first()?.parse().ok()?, r.get(3).and_then(|v| tenths_value(v))))).collect();
        let numeric = rates.iter().all(|(_, v)| v.is_some());
        let spinner = rates.iter().find(|(t, _)| *t == busy).and_then(|(_, v)| *v);
        let top = rates.iter().filter(|(t, _)| *t != busy).filter_map(|(_, v)| *v).max();
        let tids = row_tids(&rows2);
        back_to_cells(o, load.l1)?;
        let detail = format!(
            "cell={} samples={sample}->{next} first_all_new={fresh} numeric={numeric} rows={} busy_tid={busy} busy_tenths={spinner:?} next_tenths={top:?} tids_match={}",
            load.l1,
            rows2.len(),
            tids == members
        );
        if fresh && numeric && rows2.len() == WORKERS + 1 && tids == members && spinner.is_some_and(|v| top.is_none_or(|t| v > t)) {
            Ok(detail)
        } else {
            Err(format!("{detail} tui={tids:?} expected={members:?}"))
        }
    })();
    marks.result("top-load-drill-live", drill);

    // #10 (TUI part): parked, drill again and read the membership.
    let idle = ask(l1, &mut [&mut *o], "idle", "TOPLOAD idle")?;
    let tui = (|| -> Result<Vec<u64>, String> {
        press(o, ENTER)?;
        let (sample, _) = drill_frame(o, load.l1, None)?;
        let (_, rows) = drill_frame(o, load.l1, Some(sample))?;
        back_to_cells(o, load.l1)?;
        Ok(row_tids(&rows))
    })();
    // #11: lifecycle keys on L1's highlighted row, then quit.
    let selected = o.screen.frame().and_then(|f| f.selected_id());
    let keys = type_keys(o, "k").and_then(|_| type_keys(o, "K")).and_then(|_| press(o, DELETE));
    drain_for(o, 2_000);
    let still = o.screen.frame().is_some() && o.host.poll_child() == Ok(ChildPoll::Running);
    let quit = quit_tui(o, &mut [], "sttui10");
    recover_prompt(o);
    // #10: batch and records of the same incarnation.
    let batch = o.run(&format!("top -b -n 1 --cell {}", load.l1), CAPTURE_MS).map(|lines| first_ints(&lines));
    let records = capture(o, &format!("top --records --cell {} --frames 1 | to jsonl", load.l1)).map(|found| {
        let rows = found[0].tasks();
        let owned = rows.iter().all(|t| t.owner == Some(load.l1));
        (sorted(rows.iter().map(|t| t.tid).collect::<Vec<_>>()), owned)
    });
    let agree = matches!((&tui, &batch, &records), (Ok(t), Ok(b), Ok((r, true))) if *t == members && *b == members && *r == members);
    let detail = format!(
        "cell={} tui={:?} batch={:?} records={:?} expected={}",
        load.l1,
        tui.as_ref().map(Vec::len),
        batch.as_ref().map(Vec::len),
        records.as_ref().map(|(r, owned)| (r.len(), *owned)),
        members.len()
    );
    marks.result("top-load-views-agree", if agree && quit.is_ok() { Ok(detail) } else { Err(format!("{detail} quit={quit:?} tui={tui:?} batch={batch:?}")) });

    // #11: nothing reached L1 between `idle` and `ping`, all 31 workers live, same TIDs.
    l1.pump();
    let quiet = strip_ansi(&l1.out) == format!("{idle}\n");
    let pong = ask(l1, &mut [], "ping", "TOPLOAD pong");
    let live = pong.as_ref().ok().and_then(|line| ack_uint(line, "live"));
    let same = matches!(&records, Ok((r, true)) if *r == members);
    let detail = format!(
        "selected={selected:?} keys={keys:?} still_drawing={still} l1_quiet={quiet} live={live:?} tids_unchanged={same}"
    );
    let ok = selected.as_deref() == Some(load.l1.to_string().as_str()) && keys.is_ok() && still && quiet && live == Some(WORKERS as u64) && same;
    marks.result("top-load-no-lifecycle", if ok { Ok(detail) } else { Err(detail) });
    Ok(())
}

/// #12: a spinner exits during a task stream of L1. Returns the `cpu_ns` sum of L1's task
/// rows in the last frame before the exit (the floor #13 compares with).
fn exit_gone(o: &mut Term, l1: &mut Term, load: &Load, marks: &mut Marks) -> Result<u64, String> {
    ask(l1, &mut [], "busy 2", "TOPLOAD busy")?;
    sleep_ms(3_000);
    let cmd = format!("top --records --cell {} -a -d 1 --frames 4 | to jsonl", load.l1);
    let (found, exited) = stream_across(o, &cmd, |o| ask(l1, &mut [&mut *o], "exit 1", "TOPLOAD exited"))?;
    let tid = ack_tids(&exited).filter(|t| t.len() == 1).ok_or_else(|| format!("exit ack {exited:?}"))?[0];
    let shape: Vec<String> = found
        .iter()
        .map(|f| format!("{}t{:?}g{:?}", f.tasks().len(), f.total_tasks(), f.gone("gone_tasks")))
        .collect();
    let result = exit_gone_tasks(&found, tid).map(|before| (before, found[before].tasks().iter().filter_map(|t| t.cpu_ns).sum::<u64>()));
    let floor = result.as_ref().map(|(_, sum)| *sum).unwrap_or(u64::MAX);
    marks.result(
        "top-load-exit-gone-tasks",
        result.map(|(before, sum)| format!("tid={tid} last_listed_frame={before} task_cpu_ns_sum={sum} frames={shape:?}")).map_err(|why| format!("tid={tid} {why} frames={shape:?}")),
    );
    Ok(floor)
}

/// #13: the other spinner exits during a Cells stream: L1 keeps its exited threads' CPU.
fn exit_retains(o: &mut Term, l1: &mut Term, load: &mut Load, floor: u64, marks: &mut Marks) -> Result<(), String> {
    let (found, exited) = stream_across(o, "top --records -a -d 1 --frames 4 | to jsonl", |o| ask(l1, &mut [&mut *o], "exit 1", "TOPLOAD exited"))?;
    load.keep(&found, false);
    let result = exit_retains_cpu(&found, load.l1, WORKERS as u64, floor)
        .map(|cpu| format!("exited={exited:?} l1_cpu_ns={cpu:?} floor={floor}"))
        .map_err(|why| format!("exited={exited:?} {why}"));
    marks.result("top-load-exit-retains-cpu", result);
    Ok(())
}

/// #14: quota once per Cell, never per task; L2's covers its in-quota stacks.
fn quota(o: &mut Term, load: &Load, marks: &mut Marks) {
    let result = capture(o, "top --records --frames 1 | to jsonl").and_then(|cells| {
        let tasks = capture(o, &format!("top --records --cell {} --frames 1 | to jsonl", load.l2))?;
        quota_once(&cells[0], &tasks[0], load.l2).map(|detail| format!("cell={} {detail}", load.l2))
    });
    marks.result("top-load-quota-once", result);
}

/// #15/#16: L1 quits, retires and is relaunched during a Cells stream. Returns the old
/// and the new incarnation.
fn relaunch(o: &mut Term, l1: &mut Term, load: &mut Load, marks: &mut Marks) -> Result<(Ident, Ident), String> {
    let old = load.l1;
    let (found, (status, polls, tids)) = stream_across(o, "top --records -a -d 1 --frames 20 | to jsonl", |o| {
        let status = quit(l1, &mut [&mut *o], "stq1")?;
        let polls = wait_retired(l1, &mut [&mut *o], old)?;
        let tids = launch(l1, &mut [&mut *o], RELAUNCH_WORKERS)?;
        Ok((status, polls, tids))
    })?;
    load.keep(&found, false);
    let last = found.last().map(RecFrame::cells).unwrap_or_default();
    let fresh: Vec<Ident> = last.iter().filter(|c| c.name == "top-load" && c.ident != old && c.ident != load.l2).map(|c| c.ident).collect();
    let [new] = fresh.as_slice() else {
        let why = format!("new incarnation not found once in the last frame: {fresh:?} old={old}");
        marks.emit("top-load-relaunch", false, &why);
        marks.emit("top-load-id-reused", false, &why);
        return Err(why);
    };
    let new = *new;
    let sequence = relaunch_sequence(&found, old, new);
    let ok = status == 0;
    marks.result(
        "top-load-relaunch",
        match sequence {
            Ok(detail) if ok => Ok(format!("status={status} polls={polls} new_tids={tids:?} {detail}")),
            Ok(detail) | Err(detail) => Err(format!("status={status} polls={polls} {detail}")),
        },
    );
    let detail = format!("old={old} new={new}");
    marks.result("top-load-id-reused", if new.id == old.id && new.generation > old.generation { Ok(detail) } else { Err(detail) });
    load.l1 = new;
    Ok((old, new))
}

/// #17: a gone `--cell` shows nothing (batch: no rows, header complete; records: no row,
/// End complete) and never the reused slot's new incarnation.
fn gone_cell_empty(o: &mut Term, old: Ident, new: Ident, marks: &mut Marks) {
    let batch = o.run(&format!("top -b -n 1 --cell {old}"), CAPTURE_MS);
    let batch_status = status_on(o, &mut [], "stgone");
    let records = capture(o, &format!("top --records --cell {old} --frames 1 | to jsonl"));
    let records_status = status_on(o, &mut [], "stgonerec");
    let batch_rows = batch.as_ref().map(|lines| first_ints(lines).len());
    let header = batch.as_ref().is_ok_and(|lines| lines.iter().any(|l| l.starts_with("sample ") && l.split_whitespace().any(|w| w == "complete")));
    let new_token = format!("\"generation\":{}", new.generation);
    let (rows, complete, foreign) = match &records {
        Ok(found) => (found[0].rows.len(), found[0].complete(), found[0].rows.iter().any(|r| r.contains(&new_token))),
        Err(_) => (usize::MAX, None, false),
    };
    let detail = format!(
        "old={old} new={new} batch_rows={batch_rows:?} header_complete={header} batch_status={batch_status:?} record_rows={rows} record_complete={complete:?} records_status={records_status:?}"
    );
    let ok = batch_rows == Ok(0) && header && rows == 0 && complete == Some(true) && !foreign;
    marks.result("top-load-gone-cell-empty", if ok { Ok(detail) } else { Err(format!("{detail} records={:?}", records.err())) });
}

/// #18: the drilled view of an incarnation that exits and whose slot is reused keeps its
/// identity and shows no task (and nothing of the third incarnation); Esc lists the third.
fn tui_gone_notice(o: &mut Term, l1: &mut Term, new: Ident, marks: &mut Marks) {
    let result = (|| -> Result<String, String> {
        tui_select(o, &mut [], new)?;
        press(o, ENTER)?;
        let (sample, rows) = drill_frame(o, new, None)?;
        let status = quit(l1, &mut [&mut *o], "stq2")?;
        let polls = wait_retired(l1, &mut [&mut *o], new)?;
        launch(l1, &mut [&mut *o], RELAUNCH_WORKERS)?;
        let token = new.to_string();
        let mut last = o.screen.frame().and_then(|f| f.sample()).unwrap_or(sample);
        let mut gone_rows = Vec::new();
        for _ in 0..2 {
            let (next, rows) = wait_screen(o, 30_000, "frame after relaunch", |s| {
                let f = s.frame()?;
                let next = f.sample().filter(|&n| n > last)?;
                Some((next, f.rows()))
            })?;
            last = next;
            gone_rows = rows;
        }
        let title = o.screen.frame().map(|f| f.title()).unwrap_or_default();
        let kept = title.split_whitespace().any(|t| t == token);
        let no_tasks = !gone_rows.iter().any(|r| r.first().is_some_and(|t| t.parse::<u64>().is_ok()));
        press(o, ESCAPE)?;
        let third = wait_screen(o, 30_000, "third incarnation in the Cells view", |s| {
            s.frame()?.rows().iter().find_map(|r| {
                let id = Ident::parse(r.first()?)?;
                (r.get(1).map(String::as_str) == Some("top-load") && id.id == new.id && id.generation > new.generation).then_some(id)
            })
        });
        let third_token = third.as_ref().map(|t| t.to_string()).unwrap_or_default();
        let clean = third.is_ok() && !gone_rows.iter().flatten().any(|t| *t == third_token);
        let quit = quit_tui(o, &mut [], "sttui18");
        let detail = format!(
            "drilled={new} first_rows={} quit_status={status} polls={polls} title_kept={kept} task_rows_after={} third={third:?} third_absent_from_drill={clean} tui_quit={quit:?}",
            rows.len(),
            !no_tasks
        );
        if kept && no_tasks && clean && status == 0 && quit.is_ok() { Ok(detail) } else { Err(detail) }
    })();
    recover_prompt(o);
    marks.result("top-load-tui-gone-notice", result);
}

/// #8: over every Cells frame kept: Σ ≤ online_cpus × 1000 + Cells; Σ ≥ 800 while busy.
fn sum_bounded(load: &Load, marks: &mut Marks) {
    let sums: Vec<(Option<(u64, u64)>, bool)> = load.cells_frames.iter().map(|(f, busy)| (cpu_sum(f), *busy)).collect();
    let ok = !sums.is_empty()
        && sums.iter().all(|(sum, busy)| matches!(sum, Some((s, bound)) if s <= bound && (!busy || *s >= BUSY_SUM_MIN)))
        && sums.iter().any(|(_, busy)| *busy);
    let shown: Vec<String> = sums
        .iter()
        .map(|(sum, busy)| match sum {
            Some((s, bound)) => format!("{s}/{bound}{}", if *busy { "b" } else { "" }),
            None => String::from("?"),
        })
        .collect();
    let detail = format!("frames={} sum/bound(b=busy)={shown:?} busy_min={BUSY_SUM_MIN}", sums.len());
    marks.result("top-load-cpu-sum-bounded", if ok { Ok(detail) } else { Err(detail) });
}

/// #19: both fixtures quit with `bye` and status 0, both shells still answer, and no
/// stream showed an allocation failure.
fn teardown(o: &mut Term, l1: &mut Term, l2: &mut Term, marks: &mut Marks) {
    let first = quit(l1, &mut [], "stq3");
    let second = quit(l2, &mut [], "stq4");
    let alive1 = l1.run("echo p04-l1-alive", 30_000);
    let alive2 = l2.run("echo p04-l2-alive", 30_000);
    let oom = [&*o, &*l1, &*l2].iter().any(|t| oom_text(t));
    let detail = format!("l1_status={first:?} l2_status={second:?} l1_alive={alive1:?} l2_alive={alive2:?} oom={oom}");
    let ok = matches!(first, Ok(0))
        && matches!(second, Ok(0))
        && matches!(&alive1, Ok(l) if l == &[String::from("p04-l1-alive")])
        && matches!(&alive2, Ok(l) if l == &[String::from("p04-l2-alive")])
        && !oom;
    marks.result("top-load-teardown", if ok { Ok(detail) } else { Err(detail) });
}
