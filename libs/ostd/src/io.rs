// SPDX-License-Identifier: MPL-2.0

use crate::syscall::{sys_lookup_service, sys_recv_timeout, SyscallResult};
use crate::*;
use alloc::string::String;
use api::input::{decode_event, InputEvent, KeyState, KeySym, INPUT_EVENT_OPCODE};
use api::ipc::IPC_BUF_SIZE;
use api::syscall::service;

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
