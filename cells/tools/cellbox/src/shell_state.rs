//! Process-global shell state: output capture, pipe stdin, variables, functions.
//!
//! Everything here used to be a `static mut` or an `UnsafeCell` behind a hand-written
//! `unsafe impl Sync`, justified by "the shell is a single task". That justification
//! is true today and unenforceable tomorrow: nothing in the type system stops a
//! second task from calling `shell_print`. These are `spin::Mutex` and atomics
//! instead, which give the same `Sync` guarantee as a *checked* fact and let the
//! crate carry `#![forbid(unsafe_code)]` (F1, Spec 16 §6).
//!
//! Capacity and truncation limits are byte-for-byte the ones the fixed-size static
//! arrays imposed, so scripts that relied on them behave identically.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use api::cell_session::Status;
use core::sync::atomic::{AtomicBool, Ordering};
use ostd::io::ProcessOutput;
use ostd::sync::Mutex;

/// Variable slots. Setting a 17th variable is silently dropped, as before.
const MAX_VARS: usize = 16;
/// Shell function slots. Defining a 9th function is silently dropped, as before.
const MAX_FUNS: usize = 8;
const KEY_LIMIT: usize = 31;
const VALUE_LIMIT: usize = 127;
const BODY_LIMIT: usize = 479;

/// Stack of in-flight output captures; empty means "write to the console".
///
/// A stack rather than a saved-and-restored slot: nested captures (a `$(...)`
/// inside a pipeline stage) push and pop, and the innermost one wins.
static SINK: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
/// Bytes a pipe made available on stdin for the command currently dispatching.
static STDIN: Mutex<Vec<u8>> = Mutex::new(Vec::new());
static VARS: Mutex<Vec<(Vec<u8>, Vec<u8>)>> = Mutex::new(Vec::new());
static FUNS: Mutex<Vec<(Vec<u8>, Vec<u8>)>> = Mutex::new(Vec::new());
static EXIT_REQUEST: Mutex<Option<i32>> = Mutex::new(None);
static LOOP_SIGNAL: Mutex<LoopSignal> = Mutex::new(LoopSignal::None);
static BG_SPAWN: AtomicBool = AtomicBool::new(false);

/// Truncate to `limit` bytes, then stop at the first NUL.
///
/// Both steps mirror the retired fixed-array encoding, which wrote at most
/// `limit` bytes into the slot and NUL-terminated it — a value containing a NUL
/// was therefore already cut short on read.
fn slot_bytes(text: &str, limit: usize) -> Vec<u8> {
    let bytes = &text.as_bytes()[..text.len().min(limit)];
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    bytes[..end].to_vec()
}

/// Decode a slot the way the old reader did: invalid UTF-8 reads back as absent.
fn slot_str(bytes: &[u8]) -> Option<String> {
    core::str::from_utf8(bytes).ok().map(String::from)
}

fn put(store: &mut Vec<(Vec<u8>, Vec<u8>)>, key: Vec<u8>, value: Vec<u8>, capacity: usize) {
    if let Some(slot) = store.iter_mut().find(|(k, _)| *k == key) {
        slot.1 = value;
    } else if store.len() < capacity {
        store.push((key, value));
    }
}

// ── Output sink ──────────────────────────────────────────────────────────────

/// Route a string to the innermost capture buffer, else the one-shot stdout, else the
/// console.
pub fn write_out(s: &str) {
    write_out_bytes(s.as_bytes());
}

/// Byte-exact twin of [`write_out`]: an in-flight capture and the one-shot stdout keep
/// every byte (NUL and non-UTF-8 included). Only the legacy console, which prints text,
/// converts lossily.
pub fn write_out_bytes(bytes: &[u8]) {
    {
        let mut sink = SINK.lock();
        if let Some(buffer) = sink.last_mut() {
            buffer.extend_from_slice(bytes);
            return;
        }
    } // the console write must not hold the sink lock
    if oneshot_active() {
        // A failure is sticky and reported once by the one-shot entry (DS0-4).
        let _ = oneshot_stdout(bytes);
        return;
    }
    ostd::io::print(&String::from_utf8_lossy(bytes));
}

// ── One-shot output (S10-0.5) ────────────────────────────────────────────────

/// The output grants of a `/bin/cellbox <command>` one-shot, installed before anything is
/// written. While installed, uncaptured command output and diagnostics go to these
/// streams, never to the legacy console.
struct OneShot {
    output: ProcessOutput,
    /// First stdout failure; nothing more is written to stdout after it.
    stdout_failure: Option<Status>,
    /// Stderr failed once; later diagnostics are dropped.
    stderr_failed: bool,
}

static ONESHOT: Mutex<Option<OneShot>> = Mutex::new(None);

/// Finite per-write deadline of one-shot output: the S3-output stock-tool value.
const ONESHOT_WRITE_NS: u64 = 30_000_000_000;

/// Install the one-shot's output grants (once, before the command runs).
#[cfg(all(not(test), not(feature = "shell_test")))]
pub fn install_oneshot_output(output: ProcessOutput) {
    *ONESHOT.lock() = Some(OneShot { output, stdout_failure: None, stderr_failed: false });
}

/// True while a one-shot owns this process's output.
pub fn oneshot_active() -> bool {
    ONESHOT.lock().is_some()
}

fn write_deadline() -> Result<u64, Status> {
    let now = ostd::session::monotonic_ns()?;
    match now.checked_add(ONESHOT_WRITE_NS) {
        Some(deadline) if deadline != u64::MAX => Ok(deadline),
        _ => Err(Status::Malformed),
    }
}

/// Write `bytes` to the one-shot stdout with a finite deadline. After the first failure
/// (closed, revoked, deadline) nothing more is written: a committed prefix is never
/// replayed. Returns that failure. Not installed: `Err(WrongSession)`.
pub fn oneshot_stdout(bytes: &[u8]) -> Result<(), Status> {
    let mut slot = ONESHOT.lock();
    let Some(oneshot) = slot.as_mut() else {
        return Err(Status::WrongSession);
    };
    if let Some(failure) = oneshot.stdout_failure {
        return Err(failure);
    }
    let written = write_deadline().and_then(|deadline| oneshot.output.stdout().write_all(bytes, deadline));
    if let Err(failure) = written {
        oneshot.stdout_failure = Some(failure);
    }
    written
}

/// Best-effort diagnostic to the one-shot stderr; dropped once stderr has failed.
pub fn oneshot_stderr(bytes: &[u8]) {
    let mut slot = ONESHOT.lock();
    let Some(oneshot) = slot.as_mut() else {
        return;
    };
    if oneshot.stderr_failed {
        return;
    }
    let written = write_deadline().and_then(|deadline| oneshot.output.stderr().write_all(bytes, deadline));
    oneshot.stderr_failed = written.is_err();
}

/// The first one-shot stdout failure, if any.
#[cfg(all(not(test), not(feature = "shell_test")))]
pub fn oneshot_stdout_failure() -> Option<Status> {
    ONESHOT.lock().as_ref().and_then(|oneshot| oneshot.stdout_failure)
}

/// Take everything the innermost capture holds so far, leaving it empty and
/// active (empty when no capture is in flight).
pub fn take_capture() -> Vec<u8> {
    SINK.lock().last_mut().map(core::mem::take).unwrap_or_default()
}

/// Number of in-flight captures. A serial REPL line always holds exactly one.
pub fn capture_depth() -> usize {
    SINK.lock().len()
}

/// RAII capture of everything `write_out` receives until `finish` or drop (Law 8).
///
/// Dropping without `finish` discards the captured bytes — that is the panic /
/// early-return path, and it must still pop the stack or every later write would
/// vanish into an orphaned buffer.
pub struct CaptureGuard {
    finished: bool,
}

impl CaptureGuard {
    pub fn new() -> Self {
        SINK.lock().push(Vec::new());
        Self { finished: false }
    }

    /// Pop this capture and return its bytes.
    pub fn finish(mut self) -> Vec<u8> {
        self.finished = true;
        SINK.lock().pop().unwrap_or_default()
    }
}

impl Drop for CaptureGuard {
    fn drop(&mut self) {
        if !self.finished {
            SINK.lock().pop();
        }
    }
}

// ── Pipe stdin ───────────────────────────────────────────────────────────────

/// Publish `data` as the stdin of the command about to be dispatched.
pub fn set_stdin(data: &[u8]) {
    let mut stdin = STDIN.lock();
    stdin.clear();
    stdin.extend_from_slice(data);
}

pub fn clear_stdin() {
    STDIN.lock().clear();
}

/// A one-shot's stdin (S10-0.5): installed only for a stdin-consuming command and run
/// once, on the command's first demand, so a command that got a file operand never
/// touches the Stdin grant.
static STDIN_LOADER: Mutex<Option<fn() -> Vec<u8>>> = Mutex::new(None);

/// Defer the one-shot stdin to `loader`, called on the first [`stdin_bytes`] demand.
#[cfg(all(not(test), not(feature = "shell_test")))]
pub fn defer_stdin(loader: fn() -> Vec<u8>) {
    *STDIN_LOADER.lock() = Some(loader);
}

/// A copy of the current pipe stdin, empty when no pipe is active. A deferred one-shot
/// stdin is handed out once, like a consumed stream: a second demand reads nothing.
///
/// Returns owned bytes rather than a borrow: a borrow out of the lock cannot
/// outlive the guard, and the pointer-laundering that made the old `&'static [u8]`
/// possible is exactly what F1 forbids. Pipe payloads are shell-sized.
pub fn stdin_bytes() -> Vec<u8> {
    let loader = STDIN_LOADER.lock().take();
    if let Some(load) = loader {
        return load();
    }
    STDIN.lock().clone()
}

// ── Variables ────────────────────────────────────────────────────────────────

pub fn set_var(key: &str, value: &str) {
    put(
        &mut VARS.lock(),
        slot_bytes(key, KEY_LIMIT),
        slot_bytes(value, VALUE_LIMIT),
        MAX_VARS,
    );
}

pub fn unset_var(key: &str) {
    let key = slot_bytes(key, KEY_LIMIT);
    VARS.lock().retain(|(k, _)| *k != key);
}

pub fn get_var(key: &str) -> Option<String> {
    let key = slot_bytes(key, KEY_LIMIT);
    let vars = VARS.lock();
    vars.iter()
        .find(|(k, _)| *k == key)
        .and_then(|(_, v)| slot_str(v))
}

/// Record command and whole-pipeline completion as `$?` with the exact decimal
/// value, including statuses outside the single-digit range (124/125/126/130).
pub fn set_status(code: i32) {
    set_var("?", &alloc::format!("{}", code));
}

// ── Shell functions ──────────────────────────────────────────────────────────

pub fn define_function(name: &str, body: &str) {
    put(
        &mut FUNS.lock(),
        slot_bytes(name, KEY_LIMIT),
        slot_bytes(body, BODY_LIMIT),
        MAX_FUNS,
    );
}

pub fn get_function(name: &str) -> Option<String> {
    let name = slot_bytes(name, KEY_LIMIT);
    let funs = FUNS.lock();
    funs.iter()
        .find(|(k, _)| *k == name)
        .and_then(|(_, b)| slot_str(b))
}

// ── Control signals ──────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LoopSignal {
    None,
    Break,
    Continue,
}

pub fn set_loop_signal(signal: LoopSignal) {
    *LOOP_SIGNAL.lock() = signal;
}

/// Read and clear the pending loop signal.
pub fn take_loop_signal() -> LoopSignal {
    let mut signal = LOOP_SIGNAL.lock();
    core::mem::replace(&mut signal, LoopSignal::None)
}

pub fn request_exit(code: i32) {
    *EXIT_REQUEST.lock() = Some(code);
}

/// Read and clear a pending `exit` request.
#[cfg(not(feature = "shell_test"))] // reason: only the REPL loop can honour an exit request
pub fn take_exit_request() -> Option<i32> {
    EXIT_REQUEST.lock().take()
}

/// True while executing the command of an `Ast::Background` (`cmd &`).
///
/// A backgrounded EXTERNAL cell (e.g. `httpd 9092 /file &`) must NOT be
/// `sys_wait`'d — httpd loops forever, so waiting parks the shell in `sys_wait`
/// indefinitely and no subsequent command runs (the symptom: a second `vwrite`
/// after `httpd &` never executed, so httpd kept serving stale content). When
/// set, `spawn_external` returns right after spawn. Built-ins are unaffected —
/// they run synchronously in the shell task either way.
pub fn bg_spawn() -> bool {
    BG_SPAWN.load(Ordering::SeqCst)
}

pub fn set_bg_spawn(value: bool) {
    BG_SPAWN.store(value, Ordering::SeqCst);
}
