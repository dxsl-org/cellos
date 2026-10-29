// SPDX-License-Identifier: MPL-2.0

use crate::syscall::{sys_lookup_service, sys_recv_timeout, SyscallResult};
use crate::*;
use alloc::string::String;
use api::input::{decode_event, InputEvent, KeyState, KeySym, INPUT_EVENT_OPCODE};
use api::ipc::IPC_BUF_SIZE;
use api::syscall::service;
use api::cell_session::{ObjectRefV1, Status, TRANSFER_MAX};
use crate::child_context::{ChildContext, GrantSlot};

// ─── embedded-io glue ────────────────────────────────────────────────────────

/// Opaque I/O error wrapping a [`ViError`] for [`embedded_io`] trait impls.
///
/// `ViError` lives in `libs/types`; implementing a foreign trait on a foreign type
/// violates the orphan rule. This newtype lives in ostd and bridges the two.
#[derive(Debug)]
pub struct OstdError(pub ViError);

impl core::fmt::Display for OstdError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl core::error::Error for OstdError {}

impl embedded_io::Error for OstdError {
    fn kind(&self) -> embedded_io::ErrorKind {
        match self.0 {
            ViError::NotFound => embedded_io::ErrorKind::NotFound,
            ViError::PermissionDenied => embedded_io::ErrorKind::PermissionDenied,
            ViError::OutOfMemory => embedded_io::ErrorKind::OutOfMemory,
            ViError::WouldBlock => embedded_io::ErrorKind::Other,
            _ => embedded_io::ErrorKind::Other,
        }
    }
}

struct FixedLogLine {
    bytes: [u8; 192],
    len: usize,
}

impl FixedLogLine {
    const fn new() -> Self {
        Self {
            bytes: [0; 192],
            len: 0,
        }
    }

    fn as_str(&self) -> &str {
        // Every appended fragment originated as valid UTF-8 and is copied whole.
        core::str::from_utf8(&self.bytes[..self.len]).expect("fixed log line is UTF-8")
    }
}

impl core::fmt::Write for FixedLogLine {
    fn write_str(&mut self, value: &str) -> core::fmt::Result {
        let end = self.len.checked_add(value.len()).ok_or(core::fmt::Error)?;
        if end > self.bytes.len() {
            return Err(core::fmt::Error);
        }
        self.bytes[self.len..end].copy_from_slice(value.as_bytes());
        self.len = end;
        Ok(())
    }
}

/// Format into a bounded stack buffer and publish it through one log syscall.
///
/// Returns `false` without publishing if the formatted record exceeds 192 bytes.
pub fn print_fmt(args: core::fmt::Arguments<'_>) -> bool {
    use core::fmt::Write;

    let mut line = FixedLogLine::new();
    if line.write_fmt(args).is_err() {
        return false;
    }
    print(line.as_str());
    true
}

/// Print to console.
pub fn print(s: &str) {
    let _ = syscall::sys_log(s);
}

/// Print line to console.
pub fn println(s: &str) {
    print(s);
    print("\n");
}

pub fn print_usize(n: usize) {
    let mut buf = [0u8; 20];
    let mut i = 0;
    let mut num = n;

    if num == 0 {
        print("0");
        return;
    }

    while num > 0 {
        buf[i] = (num % 10) as u8 + b'0';
        num /= 10;
        i += 1;
    }

    // Reverse
    let mut start = 0;
    let mut end = i - 1;
    while start < end {
        buf.swap(start, end);
        start += 1;
        end -= 1;
    }

    if let Ok(s) = core::str::from_utf8(&buf[..i]) {
        print(s);
    }
}

/// Output grants captured once from this process's immutable invocation context.
///
/// Only a context-free (`Denied`) launch uses the legacy synchronous logger.
/// A committed context must supply both stdout and stderr; errors never fall
/// back to logging. This object retains no parsed context or service identity.
pub struct ProcessOutput {
    stdout: OutputStream,
    stderr: OutputStream,
}

/// One process output target, deliberately without a public raw-handle accessor.
pub struct OutputStream {
    target: OutputTarget,
}

enum OutputTarget {
    Legacy,
    Context { operation: ObjectRefV1, endpoint: ObjectRefV1 },
}

impl ProcessOutput {
    /// Read own context once and capture its independently routed output grants.
    pub fn read_own() -> Result<Self, Status> {
        Self::from_context_result(ChildContext::read_own())
    }

    fn from_context_result(context: Result<ChildContext, Status>) -> Result<Self, Status> {
        let context = match context {
            Ok(context) => context,
            Err(Status::Denied) => return Ok(Self {
                stdout: OutputStream { target: OutputTarget::Legacy },
                stderr: OutputStream { target: OutputTarget::Legacy },
            }),
            Err(error) => return Err(error),
        };
        let mut stdout = None;
        let mut stderr = None;
        for grant in context.grants() {
            if grant.kind != 1 {
                continue;
            }
            match grant.slot {
                GrantSlot::Stdout => stdout = Some(grant.object),
                GrantSlot::Stderr => stderr = Some(grant.object),
                _ => {},
            }
        }
        let operation = context.operation();
        Ok(Self {
            stdout: OutputStream { target: OutputTarget::Context {
                operation, endpoint: stdout.ok_or(Status::Denied)?,
            } },
            stderr: OutputStream { target: OutputTarget::Context {
                operation, endpoint: stderr.ok_or(Status::Denied)?,
            } },
        })
    }

    pub fn stdout(&self) -> &OutputStream {
        &self.stdout
    }

    pub fn stderr(&self) -> &OutputStream {
        &self.stderr
    }
}

impl OutputStream {
    /// Write bytes using one finite, nonzero absolute monotonic deadline.
    ///
    /// Context output preserves arbitrary bytes and waits inside the kernel for
    /// backpressure. An error can follow a committed prefix: terminate rather
    /// than replaying the original slice. Legacy output accepts only UTF-8.
    pub fn write_all(&self, bytes: &[u8], deadline_ns: u64) -> Result<(), Status> {
        if deadline_ns == 0 || deadline_ns == u64::MAX {
            return Err(Status::Malformed);
        }
        if bytes.is_empty() {
            return Ok(());
        }
        match &self.target {
            OutputTarget::Legacy => {
                let text = core::str::from_utf8(bytes).map_err(|_| Status::UnsupportedInput)?;
                let _ = syscall::sys_log(text);
                Ok(())
            },
            OutputTarget::Context { operation, endpoint } => {
                write_all_chunks(bytes, deadline_ns, |chunk, deadline| {
                    crate::session::write_stream(*operation, *endpoint, chunk, deadline)
                })
            },
        }
    }
}

fn write_all_chunks(
    mut bytes: &[u8],
    deadline_ns: u64,
    mut write: impl FnMut(&[u8], u64) -> Result<usize, Status>,
) -> Result<(), Status> {
    while !bytes.is_empty() {
        let chunk = &bytes[..bytes.len().min(TRANSFER_MAX)];
        let actual = write(chunk, deadline_ns)?;
        if actual == 0 || actual > chunk.len() {
            return Err(Status::Malformed);
        }
        bytes = &bytes[actual..];
    }
    Ok(())
}

pub struct Stdin;

impl Stdin {
    /// Read one line from stdin, echoing characters as they arrive.
    ///
    /// When the input service is online (registered via `sys_lookup_service`),
    /// reads via the input-service IPC path so that the caller participates in
    /// the focus model. Callers must have called `ostd::input::request_focus()`
    /// at least once before this returns meaningful data; without focus the input
    /// service will not dispatch key events to this cell.
    ///
    /// Falls back to the kernel `sys_read(fd=0)` path when the input service is
    /// not yet registered (early boot, or input cell not present).
    pub fn read_line(&self, buf: &mut String) -> ViResult<usize> {
        if sys_lookup_service(service::INPUT).is_some() {
            // Input-service path: receive InputEvent frames dispatched to this cell.
            // Uses a per-iteration timeout only to detect input-service restarts —
            // we re-request focus only when the TID changes, NOT on every timeout.
            // Re-requesting focus on every timeout created a ~10ms dead zone where
            // shell was in sys_send(SetFocus) and sys_try_send drops from dispatcher
            // would cause burst keystrokes to be silently lost (G18 fix regression).
            const TIMEOUT_TICKS: u64 = 20; // ~200ms between TID-change checks
            let mut bytes_read: usize = 0;
            let mut focused_on_tid: usize = 0; // 0 = no focus yet
                                               // Re-check the input service TID on each iteration — it may have
                                               // restarted. If it disappears entirely, fall through to sys_read.
            while let Some(input_tid) = sys_lookup_service(service::INPUT) {
                // Request focus only on startup or when input service restarted (TID changed).
                if focused_on_tid != input_tid {
                    if !crate::input::request_focus() {
                        // Input service up but SetFocus failed — service may be
                        // initialising.  Try again next iteration.
                        continue;
                    }
                    focused_on_tid = input_tid;
                }

                let mut frame = [0u8; IPC_BUF_SIZE];
                match sys_recv_timeout(0, &mut frame, TIMEOUT_TICKS) {
                    SyscallResult::Ok(0) => {
                        // Timeout — loop back to check if TID changed (restart detection).
                        // Do NOT reset focused_on_tid here — that would trigger a SetFocus
                        // sys_send dead zone on every 200ms timeout, causing dropped chars.
                    }
                    SyscallResult::Ok(sender) if sender == input_tid => {
                        if frame[0] != INPUT_EVENT_OPCODE {
                            continue;
                        }
                        let Some(ev) = decode_event(&frame[1..]) else {
                            continue;
                        };
                        if let InputEvent::Key(k) = ev {
                            if k.state != KeyState::Pressed {
                                continue;
                            }
                            match k.keysym {
                                KeySym::Return => {
                                    print("\n");
                                    buf.push('\n');
                                    return Ok(bytes_read + 1);
                                }
                                KeySym::Backspace if !buf.is_empty() => {
                                    print("\x08 \x08");
                                    buf.pop();
                                    bytes_read = bytes_read.saturating_sub(1);
                                }
                                KeySym::Printable if k.character > 0 => {
                                    if let Some(ch) = char::from_u32(k.character) {
                                        let mut tmp = [0u8; 4];
                                        let s = ch.encode_utf8(&mut tmp);
                                        print(s);
                                        buf.push(ch);
                                        bytes_read += 1;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    SyscallResult::Ok(_other) => {
                        // Unexpected sender — discard and keep waiting.
                    }
                    _ => return Err(ViError::IO),
                }
            }
        }
        {
            // Fallback: input service not online; use direct kernel UART buffer.
            let mut bytes_read: usize = 0;
            loop {
                let mut c = [0u8; 1];
                if let Ok(n) = syscall::sys_read(0, &mut c) {
                    if n > 0 {
                        let ch = c[0] as char;
                        if ch == '\r' || ch == '\n' {
                            print("\n");
                            buf.push('\n');
                            return Ok(bytes_read + 1);
                        }
                        if c[0] == 8 || c[0] == 127 {
                            if !buf.is_empty() {
                                print("\x08 \x08");
                                buf.pop();
                                bytes_read = bytes_read.saturating_sub(1);
                            }
                            continue;
                        }
                        let mut tmp = [0u8; 4];
                        let s = ch.encode_utf8(&mut tmp);
                        print(s);
                        buf.push(ch);
                        bytes_read += 1;
                    }
                } else {
                    return Err(ViError::IO);
                }
            }
        }
    }
}

pub fn stdin() -> Stdin {
    Stdin
}

// ─── embedded-io trait impls ─────────────────────────────────────────────────

impl embedded_io::ErrorType for Stdin {
    type Error = OstdError;
}

impl embedded_io::Read for Stdin {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, OstdError> {
        syscall::sys_read(0, buf).map_err(|_| OstdError(ViError::IO))
    }
}

/// Handle to the standard output stream.
pub struct Stdout;

pub fn stdout() -> Stdout {
    Stdout
}

impl embedded_io::ErrorType for Stdout {
    type Error = OstdError;
}

impl embedded_io::Write for Stdout {
    fn write(&mut self, buf: &[u8]) -> Result<usize, OstdError> {
        match core::str::from_utf8(buf) {
            Ok(s) => {
                syscall::sys_log(s); // always succeeds; return value is ignored
                Ok(buf.len())
            }
            Err(_) => Err(OstdError(ViError::InvalidInput)),
        }
    }

    fn flush(&mut self) -> Result<(), OstdError> {
        Ok(()) // sys_log is synchronous; no buffering to flush
    }
}

#[cfg(test)]
mod output_tests {
    use super::*;
    use crate::child_context::Grant;

    const OPERATION: ObjectRefV1 = ObjectRefV1 { id: 1, generation: 7 };
    const STDOUT: ObjectRefV1 = ObjectRefV1 { id: 2, generation: 8 };
    const STDERR: ObjectRefV1 = ObjectRefV1 { id: 3, generation: 9 };

    fn context(stdout: bool, stderr: bool) -> ChildContext {
        let mut grants = alloc::vec::Vec::new();
        if stdout {
            grants.push(Grant::endpoint(GrantSlot::Stdout, STDOUT, 2).unwrap());
        }
        if stderr {
            grants.push(Grant::endpoint(GrantSlot::Stderr, STDERR, 2).unwrap());
        }
        ChildContext::new(OPERATION, &["echo"], b"", &[], &grants).unwrap()
    }

    #[test]
    fn only_denied_context_read_selects_legacy_output() {
        let output = ProcessOutput::from_context_result(Err(Status::Denied)).unwrap();
        assert!(matches!(&output.stdout().target, OutputTarget::Legacy));
        assert!(matches!(&output.stderr().target, OutputTarget::Legacy));
        for error in [Status::Malformed, Status::TimedOut, Status::Cancelled,
            Status::Revoked, Status::Stale, Status::Io, Status::Indeterminate]
        {
            assert_eq!(ProcessOutput::from_context_result(Err(error)).err(), Some(error));
        }
    }

    #[test]
    fn committed_context_requires_both_output_grants() {
        for (stdout, stderr) in [(false, false), (true, false), (false, true)] {
            assert_eq!(
                ProcessOutput::from_context_result(Ok(context(stdout, stderr))).err(),
                Some(Status::Denied),
            );
        }
        let output = ProcessOutput::from_context_result(Ok(context(true, true))).unwrap();
        for (stream, expected) in [(output.stdout(), STDOUT), (output.stderr(), STDERR)] {
            match &stream.target {
                OutputTarget::Context { operation, endpoint } => {
                    assert_eq!(*operation, OPERATION);
                    assert_eq!(*endpoint, expected);
                },
                OutputTarget::Legacy => panic!("committed context selected logger"),
            }
        }
    }

    #[test]
    fn empty_output_never_calls_owner_and_deadlines_are_bounded() {
        let output = ProcessOutput::from_context_result(Ok(context(true, true))).unwrap();
        let legacy = ProcessOutput::from_context_result(Err(Status::Denied)).unwrap();
        for stream in [output.stdout(), output.stderr(), legacy.stdout(), legacy.stderr()] {
            assert_eq!(stream.write_all(b"", 1), Ok(()));
            for deadline in [0, u64::MAX] {
                assert_eq!(stream.write_all(b"", deadline), Err(Status::Malformed));
                assert_eq!(stream.write_all(b"not published", deadline), Err(Status::Malformed));
            }
        }
    }

    #[test]
    fn legacy_rejects_binary_output_without_logging() {
        let output = ProcessOutput::from_context_result(Err(Status::Denied)).unwrap();
        assert_eq!(output.stdout().write_all(b"prefix\xffsuffix", 1), Err(Status::UnsupportedInput));
        assert_eq!(output.stderr().write_all(b"\xc3", 1), Err(Status::UnsupportedInput));
    }

    #[test]
    fn partial_writes_preserve_bytes_and_one_absolute_deadline() {
        let bytes: alloc::vec::Vec<u8> = (0..1300).map(|i| (i % 256) as u8).collect();
        let mut received = alloc::vec::Vec::new();
        let mut calls = 0;
        assert_eq!(write_all_chunks(&bytes, 12345, |chunk, deadline| {
            calls += 1;
            assert_eq!(deadline, 12345);
            assert!(!chunk.is_empty() && chunk.len() <= TRANSFER_MAX);
            let actual = chunk.len().min(73);
            received.extend_from_slice(&chunk[..actual]);
            Ok(actual)
        }), Ok(()));
        assert!(calls > 3);
        assert_eq!(received, bytes);
    }

    #[test]
    fn invalid_progress_stops_without_replaying() {
        for actual in [0, 4] {
            let mut calls = 0;
            assert_eq!(write_all_chunks(b"abc", 1, |_, _| {
                calls += 1;
                Ok(actual)
            }), Err(Status::Malformed));
            assert_eq!(calls, 1);
        }
    }

    #[test]
    fn errors_after_committed_prefix_are_not_retried() {
        for error in [Status::WouldBlock, Status::TimedOut, Status::Cancelled,
            Status::Revoked, Status::BrokenPipe, Status::WrongOwner,
            Status::WrongDirection, Status::Stale, Status::Indeterminate]
        {
            let mut calls = 0;
            assert_eq!(write_all_chunks(b"abcdef", 1, |chunk, _| {
                calls += 1;
                if calls == 1 {
                    assert_eq!(chunk, b"abcdef");
                    Ok(2)
                } else {
                    assert_eq!(chunk, b"cdef");
                    Err(error)
                }
            }), Err(error));
            assert_eq!(calls, 2);
        }
    }
}
