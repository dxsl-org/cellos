//! Native record producers (`ps --records`, `ls --records`, `top --records`) and the exact-byte
//! file reader behind `cat` byte stages.
//!
//! Producers are pull-based [`RowSource`]s. Construction only validates argv — every syscall,
//! VFS request and sampling delay happens on the first pull, so pipeline admission has no effect
//! and `close()` stops all further work (a closed `top` never captures again).
//!
//! `ps` and `ls` produce finite Rows; `top --records` produces Frames (spec §1), one frame per
//! telemetry capture, so truncation (`complete`/`dropped`) travels with the data.

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use text_engine::pipeline::{Category, InterruptLatch, Item, PipelineError, RowSource, Value};
use text_engine::top::records::{frame_items, task_rows, FrameItems, TaskRows};
use text_engine::top::sample::Sampler;
use text_engine::top::text::TopError;
use text_engine::top::{parse_options, TopOptions};

use crate::top::TelemetryReader;

const PS_USAGE: &str = "Usage: ps --records [-p TID]";
const LS_USAGE: &str = "Usage: ls --records [-a] [PATH]";
const TOP_USAGE: &str =
    "Usage: top --records [--frames N] [-a] [-d SECS] [-o FIELD] [--tasks | --cell ID:GEN]";

/// What the native producer named by `argv[0]` yields: `top --records` emits Frames, every
/// other producer Rows. Decided by name only; nothing is constructed.
pub fn producer_category(argv: &[String]) -> Category {
    match argv.first().map(String::as_str) {
        Some("top") => Category::Frames,
        _ => Category::Rows,
    }
}

/// Registry entry for native row producers. `None` means `argv` is not a native record producer
/// (no `--records` flag or another command); `Some(Err(2))` reports bad arguments after the
/// usage line has been written to stderr. `rg` and `top` poll the job's interrupt `latch` while
/// they work (`rg` while it walks, `top` while it waits between frames); `ps`/`ls` ignore it.
pub fn row_producer(argv: &[String], latch: &Rc<InterruptLatch>) -> Option<Result<Box<dyn RowSource>, i32>> {
    let command = argv.first()?.as_str();
    if command == "rg" {
        return Some(match crate::rg::args::parse(argv) {
            Ok(args) if !args.stdin => crate::rg::producer(&args, latch),
            Ok(_) => {
                crate::executor::shell_stderrln("rg: `-` needs byte input: pipe bytes into it or redirect `< FILE`");
                Err(2)
            }
            Err(usage) => {
                crate::executor::shell_stderr("rg: ");
                crate::executor::shell_stderrln(&usage.0);
                Err(2)
            }
        });
    }
    if !matches!(command, "ps" | "ls" | "top") || !argv[1..].iter().any(|arg| arg == "--records") {
        return None;
    }
    let args: Vec<&str> = argv[1..].iter().map(String::as_str).collect();
    let built = match command {
        "ps" => PsSource::from_args(&args),
        "ls" => LsSource::from_args(&args),
        _ => TopSource::from_args(&args, latch),
    };
    Some(built.map_err(|usage| {
        crate::executor::shell_stderrln(usage);
        2
    }))
}

/// Read a file as exact bytes (no UTF-8 validation or replacement), bounded by the `cat` read
/// limit. A failure prints one `cat: PATH: reason` diagnostic to stderr and returns status 1.
pub fn read_file_bytes_exact(path: &str) -> Result<Vec<u8>, i32> {
    let resolved = crate::cmd_fs::resolve_shell_path(path);
    crate::cmd_fs::read_path_bytes_exact(&resolved).map_err(|reason| {
        crate::executor::shell_stderr("cat: ");
        crate::executor::shell_stderr(path);
        crate::executor::shell_stderr(": ");
        crate::executor::shell_stderrln(reason);
        1
    })
}

fn field(name: &str, value: Value) -> (String, Value) {
    (String::from(name), value)
}

fn telemetry_error(command: &str, error: TopError) -> PipelineError {
    PipelineError::Source(error.message(command))
}

// ─── ps ───────────────────────────────────────────────────────────────────────

/// `ps --records`: one capture of every task (D62-1: cumulative `cpu_ns`, Null `cpu_tenths`,
/// quality `new`), `top.task.v1` rows in TID order. An incomplete capture is an error.
struct PsSource {
    filter_tid: Option<u64>,
    rows: Option<TaskRows>,
    done: bool,
}

impl PsSource {
    fn from_args(args: &[&str]) -> Result<Box<dyn RowSource>, &'static str> {
        let mut filter_tid = None;
        let mut index = 0;
        while let Some(arg) = args.get(index).copied() {
            match arg {
                "--records" => {}
                "-p" if filter_tid.is_none() => {
                    index += 1;
                    filter_tid = Some(
                        args.get(index)
                            .and_then(|value| value.parse::<u64>().ok())
                            .ok_or(PS_USAGE)?,
                    );
                }
                _ => return Err(PS_USAGE),
            }
            index += 1;
        }
        Ok(Box::new(PsSource { filter_tid, rows: None, done: false }))
    }
}

impl RowSource for PsSource {
    fn next_item(&mut self) -> Result<Option<Item>, PipelineError> {
        if !self.done {
            self.done = true;
            let frame = crate::top::capture_tasks(self.filter_tid).map_err(|error| telemetry_error("ps", error))?;
            self.rows = Some(task_rows(frame));
        }
        Ok(self.rows.as_mut().and_then(Iterator::next).map(Item::Row))
    }

    fn close(&mut self) {
        self.done = true;
        self.rows = None;
    }
}

// ─── ls ───────────────────────────────────────────────────────────────────────

struct LsSource {
    path: String,
    all: bool,
    pending: VecDeque<Value>,
    done: bool,
}

impl LsSource {
    fn from_args(args: &[&str]) -> Result<Box<dyn RowSource>, &'static str> {
        let mut all = false;
        let mut path: Option<&str> = None;
        for arg in args.iter().copied() {
            match arg {
                "--records" => {}
                "-a" | "-A" => all = true,
                _ if arg.starts_with('-') && arg.len() > 1 => return Err(LS_USAGE),
                _ if path.is_none() => path = Some(arg),
                _ => return Err(LS_USAGE),
            }
        }
        Ok(Box::new(LsSource {
            path: String::from(path.unwrap_or("")),
            all,
            pending: VecDeque::new(),
            done: false,
        }))
    }

    fn fill(&mut self) -> Result<(), PipelineError> {
        self.done = true;
        let Some(entries) = crate::commands::ls_collect(&self.path, true, self.all) else {
            let shown = if self.path.is_empty() { "/" } else { self.path.as_str() };
            return Err(PipelineError::Source(alloc::format!(
                "ls: cannot access '{shown}': No such file or directory"
            )));
        };
        for entry in entries {
            // Size is Null only when the backend genuinely could not report it.
            let size = entry.size.map_or(Value::Null, Value::UInt);
            self.pending.push_back(Value::Record(alloc::vec![
                field("name", Value::String(entry.name)),
                field("is_dir", Value::Bool(entry.is_dir)),
                field("size", size),
            ]));
        }
        Ok(())
    }
}

impl RowSource for LsSource {
    fn next_item(&mut self) -> Result<Option<Item>, PipelineError> {
        if !self.done {
            self.fill()?;
        }
        Ok(self.pending.pop_front().map(Item::Row))
    }

    fn close(&mut self) {
        self.done = true;
        self.pending.clear();
    }
}

// ─── top ──────────────────────────────────────────────────────────────────────

/// `top --records`: the first pull seeds the baseline, waits one delay and captures; every
/// later frame waits and captures. Each capture becomes one Frame (Begin, rows, End). Ctrl-C
/// during a wait (the job latch, S10-3b) ends the stream; the consumer reports 130.
struct TopSource {
    options: TopOptions,
    frames_left: usize,
    /// Reader and baseline, created by the first pull and dropped once closed,
    /// failed or finished.
    capture: Option<(TelemetryReader, Sampler)>,
    closed: bool,
    items: Option<FrameItems>,
    latch: Rc<InterruptLatch>,
}

impl TopSource {
    fn from_args(args: &[&str], latch: &Rc<InterruptLatch>) -> Result<Box<dyn RowSource>, &'static str> {
        let mut frames: Option<usize> = None;
        let mut rest: Vec<&str> = Vec::new();
        let mut index = 0;
        while let Some(arg) = args.get(index).copied() {
            match arg {
                "--records" => {}
                "--frames" => {
                    index += 1;
                    if frames.is_some() {
                        return Err(TOP_USAGE);
                    }
                    frames = Some(
                        args.get(index)
                            .and_then(|value| value.parse::<usize>().ok())
                            .ok_or(TOP_USAGE)?,
                    );
                }
                _ => rest.push(arg),
            }
            index += 1;
        }
        let options = parse_options(&rest).map_err(|_| TOP_USAGE)?;
        if frames.is_some() && options.count.is_some() {
            return Err(TOP_USAGE);
        }
        Ok(Box::new(TopSource {
            frames_left: frames.or(options.count).unwrap_or(1),
            capture: None,
            options,
            closed: false,
            items: None,
            latch: latch.clone(),
        }))
    }

    /// Capture the next frame (seeding the baseline first on the first call).
    fn next_frame(&mut self) -> Result<Option<FrameItems>, TopError> {
        if self.closed || self.frames_left == 0 {
            self.closed = true;
            self.capture = None;
            return Ok(None);
        }
        let view = self.options.view;
        let seed = self.capture.is_none();
        let (reader, sampler) = self.capture.get_or_insert_with(|| (TelemetryReader::new(view), Sampler::new()));
        if seed {
            reader.capture(sampler, view)?;
        }
        if crate::top::wait_until(self.options.delay_ns, &mut || self.latch.poll_now())
            == crate::top::Waited::Interrupted
        {
            self.closed = true;
            self.capture = None;
            return Ok(None);
        }
        let mut frame = reader.capture(sampler, view)?;
        frame.retain(self.options.show_all, view);
        frame.sort(&self.options.sort);
        self.frames_left -= 1;
        Ok(Some(frame_items(frame)))
    }
}

impl RowSource for TopSource {
    fn next_item(&mut self) -> Result<Option<Item>, PipelineError> {
        loop {
            if let Some(item) = self.items.as_mut().and_then(Iterator::next) {
                return Ok(Some(item));
            }
            match self.next_frame() {
                Ok(Some(items)) => self.items = Some(items),
                Ok(None) => {
                    self.items = None;
                    return Ok(None);
                }
                Err(error) => {
                    self.close();
                    return Err(telemetry_error("top", error));
                }
            }
        }
    }

    fn close(&mut self) {
        self.closed = true;
        self.capture = None;
        self.items = None;
    }
}
