//! Shell adapter for `top` and `ps`: captures `TelemetrySnapshot` and drives the frames.
//!
//! Option parsing, the sampler, records and the text table model are pure and live in
//! `text_engine::top`. Here: the syscall capture, the blocking delay, the batch writer
//! (`render`) and the interactive Ratatui view (`view` state and drawing, `tui` session
//! loop).

extern crate alloc;

mod render;
pub(crate) mod tui;
mod view;

#[cfg(test)]
mod tests;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use api::telemetry::{header_flags, request, TelemetrySnapshot, CELL_LEN, HEADER_LEN, TASK_LEN};
use ostd::syscall::{self, SyscallError};
use text_engine::top::sample::{Frame, Rows, Sampler};
use text_engine::top::text::TopError;
use text_engine::top::{
    parse_options, request_flags, SortSpec, TopOptions, View, MAX_SNAPSHOT_BYTES, USAGE_STATUS,
};

/// Rows the first capture makes room for; a larger population grows the buffer once.
const INITIAL_ROWS: usize = 64;

/// One reusable snapshot buffer. It starts at header + 64 Cells (+ 64 tasks when task
/// rows are requested); after a capture without `COMPLETE` it grows once to the
/// header totals, capped at [`MAX_SNAPSHOT_BYTES`], and captures again, so a frame
/// beyond the cap is reported as truncated, never silently cut.
pub struct TelemetryReader {
    buf: Vec<u64>,
    flags: usize,
}

fn words(bytes: usize) -> usize {
    bytes.div_ceil(core::mem::size_of::<u64>())
}

fn snapshot_bytes(flags: usize, cells: usize, tasks: usize) -> usize {
    let tasks = if flags & request::TASKS != 0 { tasks } else { 0 };
    cells
        .saturating_mul(CELL_LEN)
        .saturating_add(tasks.saturating_mul(TASK_LEN))
        .saturating_add(HEADER_LEN)
        .min(MAX_SNAPSHOT_BYTES)
}

fn capture_error(error: SyscallError) -> TopError {
    match error {
        SyscallError::PermissionDenied => TopError::Denied,
        SyscallError::OutOfMemory => TopError::OutOfMemory,
        _ => TopError::Malformed,
    }
}

impl TelemetryReader {
    pub fn new(view: View) -> Self {
        let flags = request_flags(view, true);
        let buf = vec![0u64; words(snapshot_bytes(flags, INITIAL_ROWS, INITIAL_ROWS))];
        Self { buf, flags }
    }

    /// Request the rows of `view` from the next capture on. The buffer is kept; a capture
    /// that needs more room grows it once.
    pub fn set_view(&mut self, view: View) {
        self.flags = request_flags(view, true);
    }

    /// Capture one snapshot (growing the buffer once when it was too small) and
    /// observe it with `sampler`.
    pub fn capture(&mut self, sampler: &mut Sampler, view: View) -> Result<Frame, TopError> {
        let mut grown = false;
        loop {
            let written = syscall::sys_telemetry_snapshot(&mut self.buf, self.flags).map_err(capture_error)?;
            let snapshot = TelemetrySnapshot::parse(syscall::telemetry_bytes(&self.buf, written))?;
            let header = snapshot.header();
            let wanted = words(snapshot_bytes(self.flags, header.total_cells as usize, header.total_tasks as usize));
            if grown || header.has(header_flags::COMPLETE) || wanted <= self.buf.len() {
                return Ok(sampler.observe(&snapshot, view));
            }
            self.buf.resize(wanted, 0);
            grown = true;
        }
    }
}

/// `ps` and `ps --records`: one capture of every task (no delay, so CPU quality is
/// `new` with no rate, D62-1), optionally only task `tid`, in TID order. A capture that
/// dropped rows is an error: neither a table nor a row stream can carry truncation.
pub fn capture_tasks(tid: Option<u64>) -> Result<Frame, TopError> {
    let view = View::Tasks;
    let mut frame = TelemetryReader::new(view).capture(&mut Sampler::new(), view)?;
    frame.check_complete()?;
    if let (Some(tid), Rows::Tasks(rows)) = (tid, &mut frame.rows) {
        rows.retain(|row| row.tid == tid);
    }
    frame.sort(&SortSpec::new(text_engine::top::Field::Tid));
    Ok(frame)
}

/// How an interruptible wait ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waited {
    Elapsed,
    Interrupted,
}

/// Block until `delay_ns` from now has passed on the monotonic clock, parking in
/// `SetTimer(1)` steps (about one scheduler tick) instead of spinning, so `top` and
/// `sleep` are not billed for waiting. `interrupted` is polled before every park
/// (S10-3b): the first `true` ends the wait as [`Waited::Interrupted`].
pub fn wait_until(delay_ns: u64, interrupted: &mut dyn FnMut() -> bool) -> Waited {
    wait_core(delay_ns, &mut || ostd::session::monotonic_ns().ok(), &mut park_one_tick, interrupted)
}

fn park_one_tick() {
    if matches!(syscall::sys_set_timer(1), syscall::SyscallResult::Err(_)) {
        ostd::task::yield_now();
    }
}

/// The wait loop over an injected clock, park and interrupt poll. An unavailable clock
/// ends the wait as elapsed (nothing to wait against), as before S10-3b.
fn wait_core(
    delay_ns: u64,
    now: &mut dyn FnMut() -> Option<u64>,
    park: &mut dyn FnMut(),
    interrupted: &mut dyn FnMut() -> bool,
) -> Waited {
    let Some(start) = now() else {
        return Waited::Elapsed;
    };
    let deadline = start.saturating_add(delay_ns);
    loop {
        if interrupted() {
            return Waited::Interrupted;
        }
        match now() {
            Some(now) if now < deadline => park(),
            _ => return Waited::Elapsed,
        }
    }
}

/// `top` entry: usage errors and refusals exit 2, telemetry and terminal failures 1 with
/// one diagnostic, Ctrl-C in the interactive view 130 (after the screen is restored).
pub fn cmd_top(args: &[String]) -> i32 {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    let options = match parse_options(&words) {
        Ok(options) => options,
        Err(usage) => {
            crate::executor::shell_stderrln(usage);
            return USAGE_STATUS;
        }
    };
    if options.batch {
        return match run_batch(&options) {
            Ok(Waited::Elapsed) => 0,
            Ok(Waited::Interrupted) => {
                crate::executor::shell_console("^C\n");
                crate::typed::INTERRUPTED_STATUS
            }
            Err(error) => {
                crate::executor::shell_stderrln(&error.message("top"));
                error.status()
            }
        };
    }
    let outcome = tui::run(&options);
    if let Some(diagnostic) = &outcome.diagnostic {
        crate::executor::shell_stderrln(diagnostic);
    }
    if outcome.status == view::INTERRUPTED {
        crate::executor::shell_console("^C\n");
    }
    outcome.status
}

/// `top -b`: the seed capture, then `-n` frames (default 1) `-d` apart as text. Ctrl-C
/// during a wait ends the batch (S10-3b); frames already printed stay printed.
fn run_batch(options: &TopOptions) -> Result<Waited, TopError> {
    let latch = crate::executor::job_latch();
    let mut remaining = options.count.unwrap_or(1);
    let mut reader = TelemetryReader::new(options.view);
    let mut sampler = Sampler::new();
    // The seed capture only sets the baseline: a rate needs two snapshots.
    reader.capture(&mut sampler, options.view)?;
    while remaining > 0 {
        if wait_until(options.delay_ns, &mut || latch.poll_now()) == Waited::Interrupted {
            return Ok(Waited::Interrupted);
        }
        let mut frame = reader.capture(&mut sampler, options.view)?;
        frame.retain(options.show_all, options.view);
        frame.sort(&options.sort);
        render::render_frame(&frame, options, remaining);
        remaining -= 1;
    }
    Ok(Waited::Elapsed)
}
