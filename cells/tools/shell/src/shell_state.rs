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
use core::sync::atomic::{AtomicBool, Ordering};
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

/// Route a string to the innermost capture buffer, or to the console if none.
pub fn write_out(s: &str) {
    {
        let mut sink = SINK.lock();
        if let Some(buffer) = sink.last_mut() {
            buffer.extend_from_slice(s.as_bytes());
            return;
        }
    } // the console write must not hold the sink lock
    ostd::io::print(s);
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

/// A copy of the current pipe stdin, empty when no pipe is active.
///
/// Returns owned bytes rather than a borrow: a borrow out of the lock cannot
/// outlive the guard, and the pointer-laundering that made the old `&'static [u8]`
/// possible is exactly what F1 forbids. Pipe payloads are shell-sized.
pub fn stdin_bytes() -> Vec<u8> {
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
