//! The `rg` row producers (proposal §3.5).
//!
//! A pull runs at most one bounded search segment: `vigrep`'s resumable file
//! search hands rows to [`RowSink`], which queues them and asks for a pause once
//! the queue holds [`Limits::queue_rows`] rows or [`Limits::queue_bytes`] bytes
//! (always at least one row). The next pulls drain the queue; only an empty queue
//! advances the search, so the queue never exceeds the cap plus one row.
//!
//! Errors of one entry, file or ignore file are reported as they happen (one
//! `rg: PATH: reason` line on stderr, escaped like the text renderer) and the
//! walk continues; rows already delivered stay delivered. At the end one summary
//! line is reported when anything failed or was skipped as a mount. The
//! completion status is 0 (a matching line, no error), 1 (no matching line, no
//! error) or 2 (any error); an interrupt ends the producer with no status.

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use text_engine::pipeline::{typed_json_len, InterruptLatch, Item, PipelineError, RowSource, Value};
use vigrep::io;
use vigrep::regex::RegexMatcher;
use vigrep::searcher::{BinaryDetection, Searcher, Sink, SinkContext, SinkMatch};
use vigrep::walk::{
    line_submatches, Begun, FileSearch, ResumableSearch, ResumeError, ResumeInput, SearchFileError, Step,
    ViSearchTree, WalkError, WalkEvent, Walker,
};

use super::text::{escape_line, escape_path};
use super::StdinInput;
use crate::typed::StreamStop;

/// Row queue and submatch bounds of one producer.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Rows queued before the search pauses.
    pub queue_rows: usize,
    /// Payload bytes queued before the search pauses.
    pub queue_bytes: usize,
    /// Submatches of one line; more is an error for the file.
    pub max_submatches: usize,
}

/// End-of-search counters (the completion summary of the contract).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Summary {
    pub files_scanned: u64,
    pub files_binary_skipped: u64,
    /// Always 0: no tree has a link kind, nothing is followed (DV7-5).
    pub links_skipped: u64,
    pub mounts_skipped: u64,
    pub lines_scanned: u64,
    pub matching_lines: u64,
    pub matches: u64,
    pub bytes_read: u64,
    pub errors: u64,
}

/// Where diagnostic lines go (stderr in the shell, a buffer in tests).
pub type Report = Box<dyn FnMut(&str)>;

/// Typed JSONL encoding bound of one row (DV7-8, D76-5).
const MAX_ROW_JSON_BYTES: usize = text_engine::pipeline::MAX_RECORD_LINE_BYTES;

/// Rows waiting to be pulled, plus the match counters the row sink keeps.
#[derive(Default)]
struct Rows {
    queue: VecDeque<Value>,
    bytes: usize,
    matching_lines: u64,
    matches: u64,
}

impl Rows {
    fn pop(&mut self) -> Option<Value> {
        let row = self.queue.pop_front()?;
        self.bytes -= row.payload_bytes();
        Some(row)
    }

    fn clear(&mut self) {
        self.queue = VecDeque::new();
        self.bytes = 0;
    }
}

fn field(name: &str, value: Value) -> (String, Value) {
    (String::from(name), value)
}

/// One `cellbox.search.line` record in the contract's field order.
fn search_row(kind: &str, path: Option<&[u8]>, line_number: u64, offset: u64, bytes: &[u8], subs: &[(usize, usize)]) -> Value {
    let (line, terminator): (&[u8], &[u8]) = match bytes.strip_suffix(b"\n") {
        Some(line) => (line, b"\n"),
        None => (bytes, b""),
    };
    let text = match core::str::from_utf8(line) {
        Ok(text) => Value::String(String::from(text)),
        Err(_) => Value::Null,
    };
    let submatches = subs
        .iter()
        .map(|&(start, end)| {
            Value::Record(alloc::vec![field("start", Value::UInt(start as u64)), field("end", Value::UInt(end as u64))])
        })
        .collect();
    Value::Record(alloc::vec![
        field("schema", Value::String(String::from("cellbox.search.line"))),
        field("schema_version", Value::UInt(1)),
        field("kind", Value::String(String::from(kind))),
        field("path", path.map_or(Value::Null, |path| Value::Path(path.to_vec()))),
        field("line_number", Value::UInt(line_number)),
        field("absolute_offset", Value::UInt(offset)),
        field("line_bytes", Value::Bytes(line.to_vec())),
        field("text", text),
        field("terminator", Value::Bytes(terminator.to_vec())),
        field("submatches", Value::List(submatches)),
    ])
}

/// The searcher's sink: one record per match/context line, queued; `Ok(false)`
/// accepts the row and pauses the search (vigrep's resumable contract).
struct RowSink<'a> {
    matcher: &'a RegexMatcher,
    path: Option<&'a [u8]>,
    rows: &'a mut Rows,
    quiet: bool,
    limits: Limits,
}

impl RowSink<'_> {
    fn push(&mut self, row: Value) -> Result<bool, io::Error> {
        if typed_json_len(&row).map_or(true, |len| len > MAX_ROW_JSON_BYTES) {
            return Err(io::Error::other(format!("record exceeds the {}-byte bound", MAX_ROW_JSON_BYTES)));
        }
        self.rows.bytes += row.payload_bytes();
        self.rows.queue.push_back(row);
        Ok(self.rows.queue.len() < self.limits.queue_rows && self.rows.bytes < self.limits.queue_bytes)
    }
}

impl Sink for RowSink<'_> {
    type Error = io::Error;

    fn matched(&mut self, _searcher: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, io::Error> {
        let subs = line_submatches(self.matcher, mat, self.limits.max_submatches)
            .map_err(|error| io::Error::other(format!("{}", error)))?;
        if !self.quiet {
            let row = search_row(
                "match",
                self.path,
                mat.line_number().unwrap_or(0),
                mat.absolute_byte_offset(),
                mat.bytes(),
                &subs,
            );
            let more = self.push(row)?;
            self.rows.matching_lines += 1;
            self.rows.matches += subs.len() as u64;
            return Ok(more);
        }
        self.rows.matching_lines += 1;
        self.rows.matches += subs.len() as u64;
        Ok(true)
    }

    fn context(&mut self, _searcher: &Searcher, context: &SinkContext<'_>) -> Result<bool, io::Error> {
        if self.quiet {
            return Ok(true);
        }
        let row = search_row(
            "context",
            self.path,
            context.line_number().unwrap_or(0),
            context.absolute_byte_offset(),
            context.bytes(),
            &[],
        );
        self.push(row)
    }
}

/// `rg: ESCAPED_PATH: ESCAPED_REASON`.
fn diagnostic(path: &[u8], reason: &dyn core::fmt::Display) -> String {
    let mut line = Vec::from(&b"rg: "[..]);
    escape_path(path, &mut line);
    line.extend_from_slice(b": ");
    escape_line(format!("{}", reason).as_bytes(), &mut line);
    String::from_utf8(line).unwrap_or_default()
}

/// The end summary line (only reported when something failed or was skipped).
fn summary_line(summary: &Summary) -> String {
    format!(
        "rg: summary: files_scanned={} files_binary_skipped={} links_skipped={} mounts_skipped={} \
         lines_scanned={} matching_lines={} matches={} bytes_read={} errors={} complete=false reason={}",
        summary.files_scanned,
        summary.files_binary_skipped,
        summary.links_skipped,
        summary.mounts_skipped,
        summary.lines_scanned,
        summary.matching_lines,
        summary.matches,
        summary.bytes_read,
        summary.errors,
        if summary.errors > 0 { "errors" } else { "mounts_skipped" },
    )
}

/// 2 after any error, else 0 with a matching line, else 1.
fn status(summary: &Summary) -> i32 {
    if summary.errors > 0 {
        2
    } else if summary.matching_lines > 0 {
        0
    } else {
        1
    }
}

/// What the producer does on its next pull.
enum Phase<F> {
    Walking,
    InFile(FileSearch<F>),
    Done,
}

/// Tree mode: walk the roots and search every admitted file.
pub struct TreeSource<T: ViSearchTree> {
    tree: T,
    walker: Walker,
    searcher: Searcher,
    matcher: RegexMatcher,
    quiet: bool,
    limits: Limits,
    latch: Rc<InterruptLatch>,
    report: Report,
    rows: Rows,
    phase: Phase<T::File>,
    lines_scanned: u64,
    errors: u64,
    completion: Option<i32>,
}

impl<T: ViSearchTree> TreeSource<T> {
    #[allow(clippy::too_many_arguments)] // reason: one constructor wires every compiled part of the search
    pub fn new(
        tree: T,
        walker: Walker,
        searcher: Searcher,
        matcher: RegexMatcher,
        quiet: bool,
        limits: Limits,
        latch: Rc<InterruptLatch>,
        report: Report,
    ) -> Self {
        TreeSource {
            tree,
            walker,
            searcher,
            matcher,
            quiet,
            limits,
            latch,
            report,
            rows: Rows::default(),
            phase: Phase::Walking,
            lines_scanned: 0,
            errors: 0,
            completion: None,
        }
    }

    /// The counters so far.
    pub fn summary(&self) -> Summary {
        let stats = self.walker.stats();
        Summary {
            files_scanned: stats.files_scanned,
            files_binary_skipped: stats.files_binary_skipped,
            links_skipped: 0,
            mounts_skipped: stats.mounts_skipped,
            lines_scanned: self.lines_scanned,
            matching_lines: self.rows.matching_lines,
            matches: self.rows.matches,
            bytes_read: stats.bytes_read,
            errors: self.errors,
        }
    }

    /// Rows and payload bytes still queued (host tests check the queue bound).
    #[cfg(test)]
    pub fn queued(&self) -> (usize, usize) {
        (self.rows.queue.len(), self.rows.bytes)
    }

    fn fail(&mut self, path: &[u8], reason: &dyn core::fmt::Display) {
        self.errors += 1;
        let line = diagnostic(path, reason);
        (self.report)(&line);
    }

    fn walk_error(&mut self, error: &WalkError) {
        self.fail(&error.path, &error.kind);
    }

    /// Release the open file (if any); the walker and queue go with the phase.
    fn release(&mut self) {
        if let Phase::InFile(search) = core::mem::replace(&mut self.phase, Phase::Done) {
            self.lines_scanned += search.lines_scanned();
            search.abandon(&mut self.tree);
        }
        self.rows.clear();
    }

    /// Ctrl-C: stop all work, no completion status.
    fn interrupt(&mut self) {
        self.release();
        self.completion = None;
    }

    fn finish(&mut self) {
        self.phase = Phase::Done;
        let summary = self.summary();
        if summary.errors + summary.mounts_skipped > 0 {
            let line = summary_line(&summary);
            (self.report)(&line);
        }
        self.completion = Some(status(&summary));
    }

    /// One unit of work: a search segment, a file start or a walk step.
    /// `false` when the producer ended (finished or interrupted).
    fn advance(&mut self) -> bool {
        let latch = self.latch.clone();
        let mut cancel = move || latch.check();
        match core::mem::replace(&mut self.phase, Phase::Walking) {
            Phase::Done => {
                self.phase = Phase::Done;
                false
            }
            Phase::InFile(mut search) => {
                let path = search.path().to_vec();
                let mut sink = RowSink {
                    matcher: &self.matcher,
                    path: Some(&path),
                    rows: &mut self.rows,
                    quiet: self.quiet,
                    limits: self.limits,
                };
                let step = search.step(
                    &mut self.walker,
                    &mut self.tree,
                    &mut self.searcher,
                    &self.matcher,
                    &mut sink,
                    &mut cancel,
                );
                match step {
                    Ok(Step::Paused) => {
                        self.phase = Phase::InFile(search);
                        true
                    }
                    Ok(Step::Finished) => {
                        self.lines_scanned += search.lines_scanned();
                        true
                    }
                    Err(SearchFileError::Cancelled) => {
                        self.lines_scanned += search.lines_scanned();
                        self.interrupt();
                        false
                    }
                    Err(SearchFileError::Walk(error)) => {
                        self.lines_scanned += search.lines_scanned();
                        self.walk_error(&error);
                        true
                    }
                    Err(SearchFileError::Sink(error)) => {
                        self.lines_scanned += search.lines_scanned();
                        self.fail(&path, &error);
                        true
                    }
                }
            }
            Phase::Walking => match self.walker.next(&mut self.tree, &mut cancel) {
                Some(WalkEvent::File { path, .. }) => {
                    match self.walker.begin_file(&mut self.tree, &path, &mut cancel) {
                        Ok(Begun::Binary) => true,
                        Ok(Begun::Search(search)) => {
                            self.phase = Phase::InFile(search);
                            true
                        }
                        Err(SearchFileError::Cancelled) => {
                            self.interrupt();
                            false
                        }
                        Err(SearchFileError::Walk(error)) => {
                            self.walk_error(&error);
                            true
                        }
                        Err(SearchFileError::Sink(never)) => match never {},
                    }
                }
                // Counted by the walker; reported once, in the summary (DV7-7).
                Some(WalkEvent::MountSkipped { .. }) => true,
                Some(WalkEvent::Error(error)) => {
                    self.walk_error(&error);
                    true
                }
                Some(WalkEvent::Cancelled) => {
                    self.interrupt();
                    false
                }
                None => {
                    self.finish();
                    false
                }
            },
        }
    }
}

impl<T: ViSearchTree> RowSource for TreeSource<T> {
    fn next_item(&mut self) -> Result<Option<Item>, PipelineError> {
        loop {
            if let Some(row) = self.rows.pop() {
                return Ok(Some(Item::Row(row)));
            }
            if matches!(self.phase, Phase::Done) {
                return Ok(None);
            }
            if self.latch.check() {
                self.interrupt();
                return Ok(None);
            }
            if !self.advance() && self.rows.queue.is_empty() {
                return Ok(None);
            }
        }
    }

    /// Consumer done early (`take`): release every handle; the status is 0, or 2
    /// when an error was already reported. Idempotent.
    fn close(&mut self) {
        if matches!(self.phase, Phase::Done) {
            self.rows.clear();
            return;
        }
        self.release();
        self.completion = Some(if self.errors > 0 { 2 } else { 0 });
    }

    fn completion(&self) -> Option<i32> {
        self.completion
    }
}

impl<T: ViSearchTree> Drop for TreeSource<T> {
    fn drop(&mut self) {
        self.release();
    }
}

// ─── stdin ───────────────────────────────────────────────────────────────────

/// The bytes of `rg -`: in memory (rewindable for free) or an external stream
/// (forward only, behind the search's resume window).
enum Input {
    Bytes(Vec<u8>),
    Stream { source: Box<dyn crate::typed::ByteSource>, offset: u64, ended: bool, closed: bool },
}

impl Input {
    fn close(&mut self) {
        if let Input::Stream { source, ended, closed, .. } = self {
            if !*closed {
                *closed = true;
                source.close(*ended);
            }
        }
    }
}

impl ResumeInput for Input {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<usize, io::Error> {
        match self {
            Input::Bytes(bytes) => {
                let start = usize::try_from(offset).unwrap_or(usize::MAX).min(bytes.len());
                let count = buf.len().min(bytes.len() - start);
                buf[..count].copy_from_slice(&bytes[start..start + count]);
                Ok(count)
            }
            Input::Stream { source, offset: at, ended, closed } => {
                if *ended || *closed {
                    return Ok(0);
                }
                if offset != *at {
                    return Err(io::Error::other("stream read out of order"));
                }
                let mut chunk = [0u8; crate::typed::STREAM_CHUNK];
                let want = buf.len().min(chunk.len());
                match source.read_chunk(&mut chunk[..want]) {
                    Ok(Some(count)) if count <= want => {
                        buf[..count].copy_from_slice(&chunk[..count]);
                        *at += count as u64;
                        if count == 0 {
                            *ended = true;
                        }
                        Ok(count)
                    }
                    Ok(Some(_)) => Err(io::Error::other("malformed stream read")),
                    Ok(None) => {
                        *ended = true;
                        Ok(0)
                    }
                    Err(StreamStop::Interrupted) => Err(io::Error::new(io::ErrorKind::Interrupted, "interrupted")),
                    Err(StreamStop::Failed(message)) => Err(io::Error::other(message)),
                }
            }
        }
    }

    fn rewindable(&self) -> bool {
        matches!(self, Input::Bytes(_))
    }
}

/// Stdin mode (`rg -a PATTERN -`): one resumable search over the input bytes.
pub struct StdinSource {
    input: Input,
    search: Option<ResumableSearch>,
    searcher: Searcher,
    matcher: RegexMatcher,
    quiet: bool,
    limits: Limits,
    latch: Rc<InterruptLatch>,
    report: Report,
    rows: Rows,
    errors: u64,
    completion: Option<i32>,
    lines_scanned: u64,
    bytes_read: u64,
}

impl StdinSource {
    #[allow(clippy::too_many_arguments)] // reason: one constructor wires every compiled part of the search
    pub fn new(
        input: StdinInput,
        mut searcher: Searcher,
        matcher: RegexMatcher,
        quiet: bool,
        limits: Limits,
        window: usize,
        latch: Rc<InterruptLatch>,
        report: Report,
    ) -> Self {
        // `-` requires `-a`: NUL bytes are content.
        searcher.set_binary_detection(BinaryDetection::none());
        let (input, window) = match input {
            StdinInput::Bytes(bytes) => (Input::Bytes(bytes), 0),
            StdinInput::Stream(source) => (Input::Stream { source, offset: 0, ended: false, closed: false }, window),
        };
        StdinSource {
            input,
            search: Some(ResumableSearch::new(window)),
            searcher,
            matcher,
            quiet,
            limits,
            latch,
            report,
            rows: Rows::default(),
            errors: 0,
            completion: None,
            lines_scanned: 0,
            bytes_read: 0,
        }
    }

    pub fn summary(&self) -> Summary {
        let (lines, bytes) = match &self.search {
            Some(search) => (search.lines_scanned(), search.bytes_read()),
            None => (self.lines_scanned, self.bytes_read),
        };
        Summary {
            files_scanned: u64::from(self.completion.is_some() && self.errors == 0),
            lines_scanned: lines,
            matching_lines: self.rows.matching_lines,
            matches: self.rows.matches,
            bytes_read: bytes,
            errors: self.errors,
            ..Summary::default()
        }
    }

    /// End the search: keep its counters, release the input.
    fn end(&mut self) {
        if let Some(search) = self.search.take() {
            self.lines_scanned = search.lines_scanned();
            self.bytes_read = search.bytes_read();
        }
        self.input.close();
    }

    fn fail(&mut self, reason: &dyn core::fmt::Display) {
        self.errors += 1;
        let mut line = Vec::from(&b"rg: <stdin>: "[..]);
        escape_line(format!("{}", reason).as_bytes(), &mut line);
        let line = String::from_utf8(line).unwrap_or_default();
        (self.report)(&line);
    }

    fn finish(&mut self) {
        self.end();
        let summary = self.summary();
        if summary.errors > 0 {
            let line = summary_line(&summary);
            (self.report)(&line);
        }
        self.completion = Some(status(&summary));
    }

    fn interrupt(&mut self) {
        self.end();
        self.rows.clear();
        self.completion = None;
    }
}

impl RowSource for StdinSource {
    fn next_item(&mut self) -> Result<Option<Item>, PipelineError> {
        loop {
            if let Some(row) = self.rows.pop() {
                return Ok(Some(Item::Row(row)));
            }
            if self.search.is_none() {
                return Ok(None);
            }
            if self.latch.check() {
                self.interrupt();
                return Ok(None);
            }
            let Some(search) = self.search.as_mut() else {
                return Ok(None);
            };
            let latch = self.latch.clone();
            let mut cancel = move || latch.check();
            let mut sink = RowSink {
                matcher: &self.matcher,
                path: None,
                rows: &mut self.rows,
                quiet: self.quiet,
                limits: self.limits,
            };
            match search.step(&mut self.input, &mut self.searcher, &self.matcher, &mut sink, &mut cancel) {
                Ok(Step::Paused) => {}
                Ok(Step::Finished) => self.finish(),
                Err(ResumeError::Cancelled) => {
                    self.interrupt();
                    return Ok(None);
                }
                Err(ResumeError::Io(error)) if error.kind() == io::ErrorKind::Interrupted || self.latch.is_set() => {
                    self.interrupt();
                    return Ok(None);
                }
                Err(error) => {
                    self.fail(&error);
                    self.finish();
                }
            }
        }
    }

    fn close(&mut self) {
        if self.search.is_none() {
            self.rows.clear();
            return;
        }
        self.end();
        self.rows.clear();
        self.completion = Some(if self.errors > 0 { 2 } else { 0 });
    }

    fn completion(&self) -> Option<i32> {
        self.completion
    }
}

impl Drop for StdinSource {
    fn drop(&mut self) {
        self.input.close();
    }
}

/// `rg -h`: no rows, status 0.
pub struct Empty;

impl RowSource for Empty {
    fn next_item(&mut self) -> Result<Option<Item>, PipelineError> {
        Ok(None)
    }

    fn close(&mut self) {}

    fn completion(&self) -> Option<i32> {
        Some(0)
    }
}
