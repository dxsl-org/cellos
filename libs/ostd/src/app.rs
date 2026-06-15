// SPDX-License-Identifier: MPL-2.0

//! ViCell App SDK — structured entry point for Cell applications.
//!
//! Instead of writing raw `sys_recv` dispatch loops, cells declare a handler and
//! call `AppContext::run()`. The context manages the receive buffer, interprets
//! the message envelope, and delivers typed [`AppEvent`]s.
//!
//! # Message envelope
//!
//! Messages sent through [`AppContext::send_msg`] carry a 2-byte header:
//! - byte 0: [`APP_MSG_MAGIC`] (0xAC) — namespace guard, prevents raw-protocol collisions
//! - byte 1: event type discriminant (0x00 = Message, 0xFF = Shutdown)
//! - bytes 2..: payload (caller-defined)
//!
//! Messages that do not start with `APP_MSG_MAGIC` are delivered as raw
//! [`AppEvent::RawMessage`] so legacy IPC senders remain compatible.
//!
//! # Example
//! ```no_run
//! use ostd::app::{AppContext, AppEvent};
//!
//! fn handler(ctx: &mut AppContext, ev: AppEvent) {
//!     if let AppEvent::Message { sender_tid, .. } = ev {
//!         ctx.send(sender_tid, b"\xAC\x00pong").ok();
//!     }
//! }
//!
//! AppContext::new().run(handler);
//! ```

extern crate alloc;

use alloc::vec::Vec;
use crate::{syscall, ViError, ViResult};

/// Magic byte prefixing every App SDK envelope.
///
/// Prevents collisions between typed AppContext messages and raw `sys_send` clients.
pub const APP_MSG_MAGIC: u8 = 0xAC;

const RECV_BUF_SIZE: usize = 4096;

/// Typed event delivered to an [`AppContext`] handler.
///
/// AppEvent owns its payload data so the handler can store or forward it without
/// lifetime constraints tied to the receive buffer.
#[derive(Debug)]
pub enum AppEvent {
    /// An App SDK message (envelope magic verified).
    Message {
        /// TID of the sending cell.
        sender_tid: usize,
        /// Message payload (bytes after the 2-byte header), heap-owned.
        data: Vec<u8>,
    },
    /// A raw IPC message not using the App SDK envelope (legacy / non-SDK senders).
    RawMessage {
        sender_tid: usize,
        /// Full message bytes, heap-owned.
        data: Vec<u8>,
    },
    /// A keyboard or mouse event delivered by the input service.
    ///
    /// Automatically decoded from the 65-byte `0x10`-opcode IPC message.
    /// Cells must call [`AppContext::request_input_focus`] at startup to
    /// receive these events.
    Input(api::input::InputEvent),
    /// The receive deadline elapsed (only from [`AppContext::run_with_timeout`]).
    Timeout,
    /// Kernel-requested graceful shutdown signal (future: power management).
    Shutdown,
}

/// Execution context for a Cell application.
///
/// Owns the receive buffer and drives the IPC loop.  Create once in `main()` and
/// call [`run`][AppContext::run] — it never returns unless the handler panics.
pub struct AppContext {
    recv_buf: [u8; RECV_BUF_SIZE],
}

impl AppContext {
    /// Create a new context with a stack-allocated receive buffer.
    pub fn new() -> Self {
        Self { recv_buf: [0u8; RECV_BUF_SIZE] }
    }

    /// Run the IPC event loop, calling `handler` for every incoming event.
    ///
    /// Blocks indefinitely.  The handler receives `&mut AppContext` so it can call
    /// [`send`][AppContext::send], [`lookup_service`][AppContext::lookup_service], etc.
    pub fn run(&mut self, mut handler: impl FnMut(&mut AppContext, AppEvent)) -> ! {
        loop {
            let result = syscall::sys_recv(0, &mut self.recv_buf);
            let sender_tid = match result {
                syscall::SyscallResult::Ok(tid) => tid,
                syscall::SyscallResult::Err(_) => {
                    syscall::sys_yield();
                    continue;
                }
            };
            // Parse and own the event data before calling handler so there is no
            // outstanding borrow on self.recv_buf when handler runs.
            let event = Self::parse_event_owned(sender_tid, &self.recv_buf);
            handler(self, event);
        }
    }

    /// Run the IPC event loop with a per-iteration receive timeout.
    ///
    /// `timeout_ticks` is the maximum scheduler ticks to wait (one tick ≈ 10 ms).
    /// When the deadline elapses the handler is called with [`AppEvent::Timeout`].
    pub fn run_with_timeout(
        &mut self,
        timeout_ticks: u64,
        mut handler: impl FnMut(&mut AppContext, AppEvent),
    ) -> ! {
        loop {
            let result = syscall::sys_recv_timeout(0, &mut self.recv_buf, timeout_ticks);
            let sender_tid = match result {
                syscall::SyscallResult::Ok(0) => {
                    handler(self, AppEvent::Timeout);
                    continue;
                }
                syscall::SyscallResult::Ok(tid) => tid,
                syscall::SyscallResult::Err(_) => {
                    syscall::sys_yield();
                    continue;
                }
            };
            let event = Self::parse_event_owned(sender_tid, &self.recv_buf);
            handler(self, event);
        }
    }

    /// Send raw bytes `data` to another cell by TID.
    pub fn send(&self, tid: usize, data: &[u8]) -> ViResult<()> {
        match syscall::sys_send(tid, data) {
            syscall::SyscallResult::Ok(_) => Ok(()),
            syscall::SyscallResult::Err(_) => Err(ViError::IO),
        }
    }

    /// Send an App SDK envelope message to `tid`.
    ///
    /// Prepends `[APP_MSG_MAGIC, 0x00]` so the receiver's `AppContext` decodes it
    /// as [`AppEvent::Message`].
    pub fn send_msg(&self, tid: usize, payload: &[u8]) -> ViResult<()> {
        let mut buf = [0u8; RECV_BUF_SIZE];
        if payload.len() + 2 > RECV_BUF_SIZE {
            return Err(ViError::InvalidArgument);
        }
        buf[0] = APP_MSG_MAGIC;
        buf[1] = 0x00;
        buf[2..2 + payload.len()].copy_from_slice(payload);
        self.send(tid, &buf[..2 + payload.len()])
    }

    /// Resolve the live provider tid of a well-known service.
    pub fn lookup_service(&self, service_id: u16) -> Option<usize> {
        syscall::sys_lookup_service(service_id)
    }

    /// Register this cell as the keyboard/mouse focus recipient.
    ///
    /// Delegates to [`ostd::input::request_focus`].  Call once at startup
    /// after any compositor surface is ready.  Returns `true` when the input
    /// service grants focus; `false` on boot race (retry with a yield).
    pub fn request_input_focus(&self) -> bool {
        crate::input::request_focus()
    }

    // ── Private ──────────────────────────────────────────────────────────────

    /// Parse `buf` into an owned `AppEvent`, copying the payload into a `Vec`.
    ///
    /// Owning the data eliminates the lifetime coupling between the event and the
    /// receive buffer, so the caller can pass `&mut self` to the handler freely.
    fn parse_event_owned(sender_tid: usize, buf: &[u8]) -> AppEvent {
        if buf.len() >= 2 && buf[0] == APP_MSG_MAGIC {
            let data = buf[2..].to_vec();
            match buf[1] {
                0xFF => AppEvent::Shutdown,
                _ => AppEvent::Message { sender_tid, data },
            }
        } else if buf.len() >= 2 && buf[0] == api::input::INPUT_EVENT_OPCODE {
            if let Some(ev) = crate::input::parse_frame(buf) {
                return AppEvent::Input(ev);
            }
            AppEvent::RawMessage { sender_tid, data: buf.to_vec() }
        } else {
            AppEvent::RawMessage { sender_tid, data: buf.to_vec() }
        }
    }
}

impl Default for AppContext {
    fn default() -> Self {
        Self::new()
    }
}
