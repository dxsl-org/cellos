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
//! - byte 1: event type discriminant (0x00 = Message, 0xFF = Shutdown,
//!   0xF0 = Snapshot, 0xF1 = Restore, 0xF2 = CapRevoked)
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
//!         ctx.reply(sender_tid, b"\xAC\x00pong").ok();
//!     }
//! }
//!
//! AppContext::new().run(handler);
//! ```

extern crate alloc;

use crate::{syscall, ViError, ViResult};
use alloc::vec::Vec;

/// Magic byte prefixing every App SDK envelope.
///
/// Prevents collisions between typed AppContext messages and raw `sys_send` clients.
pub const APP_MSG_MAGIC: u8 = 0xAC;

const RECV_BUF_SIZE: usize = 4096;

/// Reason delivered with a structured [`AppEvent::ShutdownWith`] event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownReason {
    /// Graceful shutdown requested by init / power management.
    Requested,
    /// Heartbeat deadline missed — kernel terminated the cell; supervisor will restart.
    Watchdog,
    /// A parent cell the cell subscribed to via `NotifyOnExit` has died.
    ParentDied,
}

/// Typed event delivered to an [`AppContext`] handler.
///
/// AppEvent owns its payload data so the handler can store or forward it without
/// lifetime constraints tied to the receive buffer.
///
/// New variants may be added in future SDK releases.  Always add a wildcard arm
/// `_ => {}` to future-proof your match.
#[non_exhaustive]
#[derive(Debug)]
pub enum AppEvent {
    /// Fires exactly once before the first `sys_recv` when using
    /// [`AppContext::run_with_lifecycle`] or [`CellRuntime::run`].
    /// Use for startup work: config read, service registration, UI init.
    Init,
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
    /// Kernel-requested graceful shutdown (legacy form — no reason byte in envelope).
    Shutdown,
    /// Structured shutdown with reason (emitted when the envelope carries a reason byte).
    ShutdownWith {
        /// Why the shutdown is happening.
        reason: ShutdownReason,
    },

    // ── Hot-swap lifecycle events (Phase 20) ─────────────────────────────────
    /// Kernel-sent signal to the OLD cell during Step 2 of a hot-swap sequence.
    ///
    /// The cell must serialize its state, stash it under `swap_id` as the key
    /// via [`ostd::syscall::sys_state_stash`], then yield or exit.  The kernel
    /// polls the stash; once the bytes appear the swap proceeds to Step 3.
    ///
    /// Default arm: cells that do not implement hot-swap state transfer may
    /// ignore this event — the swap continues with an empty stash (no state restored).
    Snapshot {
        /// Monotonically increasing swap identifier assigned by the kernel orchestrator.
        /// Must be used as the stash key so the replacement instance can recover it.
        swap_id: u64,
    },

    /// Kernel-sent signal to the NEW cell during Step 4 of a hot-swap sequence.
    ///
    /// The cell must restore its state from the stash under `key` via
    /// [`ostd::syscall::sys_state_restore`], then call
    /// [`ostd::syscall::sys_hotswap_ready`] to signal completion.
    ///
    /// `key` is a null-terminated UTF-8 string encoding the decimal `swap_id`,
    /// held in a fixed 64-byte buffer.  Parse with
    /// `core::str::from_utf8(&key[..key.iter().position(|&b| b==0).unwrap_or(64)])`
    /// and convert via `str::parse::<u64>()`.
    Restore {
        /// Fixed-size stash key (null-terminated, decimal swap_id).
        key: [u8; 64],
    },

    // ── Runtime authority events ─────────────────────────────────────────────
    /// The kernel revoked capabilities from this cell at runtime.
    ///
    /// Sent after the kernel has already torn the authority down: a revoked
    /// MMIO window is unmapped and a revoked DMA domain is drained, so the cell
    /// cannot keep using what it lost. The event is a courtesy — drop the
    /// subsystem that depended on the capability and stop retrying it, rather
    /// than discovering the loss through a fault.
    ///
    /// `mask` uses the `api::syscall::cap_mask` bits the kernel revoked.
    CapRevoked {
        /// The revoked capability mask.
        mask: u32,
    },
}

/// Execution context for a Cell application.
///
/// Owns the receive buffer and drives the IPC loop.  Create once in `main()` and
/// call [`run`][AppContext::run] — it never returns unless the handler panics.
///
/// Lazy service client accessors (`.vfs()`, `.net()`, `.input()`) initialize on
/// first use and are re-used across handler invocations.
pub struct AppContext {
    recv_buf: [u8; RECV_BUF_SIZE],
    vfs_client: Option<crate::clients::VfsClient>,
    net_client: Option<crate::clients::NetClient>,
    input_client: Option<crate::clients::InputClient>,
    /// Armed heartbeat interval in ticks; 0 = no watchdog.
    heartbeat_ticks: u64,
    /// Opted in with [`Self::attest_callers`]: untimed receives ask the kernel
    /// for a caller-identity trailer.
    attest_callers: bool,
    /// Kernel-attested identity of the sender of the event being handled.
    caller: Option<api::caller_identity::CallerIdentity>,
    /// The request being handled, captured at its receive: a handler that does
    /// its own IPC before replying (a receive replaces the kernel's record of
    /// the current request) still answers the caller's exact operation.
    request: Option<crate::ipc::Incoming>,
}

impl AppContext {
    /// Create a new context with a stack-allocated receive buffer.
    pub fn new() -> Self {
        Self {
            recv_buf: [0u8; RECV_BUF_SIZE],
            vfs_client: None,
            net_client: None,
            input_client: None,
            heartbeat_ticks: 0,
            attest_callers: false,
            caller: None,
            request: None,
        }
    }

    /// Ask the kernel to attest the sender of every later event
    /// (`api::caller_identity`). Call from the `Init` arm, before the first
    /// receive. The last `CALLER_IDENTITY_LEN` bytes of the receive buffer then
    /// belong to the kernel and are cut from every event's payload. Only untimed waits are attested: with a heartbeat
    /// or a timeout armed, [`Self::caller`] is `None`, which a service must treat
    /// as an unknown caller and refuse.
    pub fn attest_callers(&mut self) {
        self.attest_callers = true;
    }

    /// The receive buffer minus the kernel-owned trailer when attestation is on,
    /// so a handler never sees trailer bytes as payload.
    fn payload(&self) -> &[u8] {
        if self.attest_callers {
            &self.recv_buf[..RECV_BUF_SIZE - api::caller_identity::CALLER_IDENTITY_LEN]
        } else {
            &self.recv_buf
        }
    }

    /// Kernel-attested identity of the sender of the event being handled, or
    /// `None` when attestation is off, the wait was timed, or the kernel wrote
    /// no valid trailer. `None` means "unknown caller": deny.
    pub fn caller(&self) -> Option<api::caller_identity::CallerIdentity> {
        self.caller
    }

    /// One bounded wait of the event loop: `Some(Some(tid))` an event, `Some(None)`
    /// the wait elapsed, `None` a receive error.
    ///
    /// With a heartbeat armed the kernel only extends the deadline on an explicit
    /// `Heartbeat` syscall, so the loop re-arms it before every wait and never
    /// blocks longer than half the interval. An idle service therefore stays
    /// alive, while a handler that hangs for a whole interval is still killed.
    fn wait_once(&mut self, max_ticks: Option<u64>) -> Option<Option<usize>> {
        let wait = match (self.heartbeat_ticks, max_ticks) {
            (0, None) => None,
            (0, Some(t)) => Some(t),
            (h, None) => Some((h / 2).max(1)),
            (h, Some(t)) => Some(t.min((h / 2).max(1))),
        };
        if self.heartbeat_ticks > 0 {
            syscall::sys_heartbeat(self.heartbeat_ticks);
        }
        self.caller = None;
        let attested = self.attest_callers && wait.is_none();
        if attested {
            // A message the kernel does not attest must not inherit the
            // previous sender's trailer.
            let tail = RECV_BUF_SIZE - api::caller_identity::CALLER_IDENTITY_LEN;
            self.recv_buf[tail..].fill(0);
        }
        let result = match wait {
            None if attested => syscall::sys_recv_attested(0, &mut self.recv_buf),
            None => syscall::sys_recv(0, &mut self.recv_buf),
            Some(ticks) => syscall::sys_recv_timeout(0, &mut self.recv_buf, ticks),
        };
        if attested && matches!(result, syscall::SyscallResult::Ok(tid) if tid != 0) {
            self.caller = api::caller_identity::CallerIdentity::from_recv_buf(&self.recv_buf);
        }
        self.request = match result {
            syscall::SyscallResult::Ok(tid) if tid != 0 => Some(crate::ipc::Incoming::current(tid)),
            _ => None,
        };
        match result {
            syscall::SyscallResult::Ok(0) if wait.is_some() => Some(None),
            syscall::SyscallResult::Ok(tid) => Some(Some(tid)),
            syscall::SyscallResult::Err(_) => None,
        }
    }

    /// Run the IPC event loop, calling `handler` for every incoming event.
    ///
    /// Blocks until events arrive. The handler receives `&mut AppContext` so it can
    /// call [`reply`][AppContext::reply], [`lookup_service`][AppContext::lookup_service],
    /// etc. An armed heartbeat is kept alive while idle (see [`Self::arm_heartbeat`]).
    pub fn run(&mut self, mut handler: impl FnMut(&mut AppContext, AppEvent)) -> ! {
        loop {
            let sender_tid = match self.wait_once(None) {
                Some(Some(tid)) => tid,
                Some(None) => continue,
                None => {
                    syscall::sys_yield();
                    continue;
                }
            };
            // Parse and own the event data before calling handler so there is no
            // outstanding borrow on self.recv_buf when handler runs.
            let event = Self::parse_event_owned(sender_tid, self.payload());
            handler(self, event);
        }
    }

    /// Run the IPC event loop with a per-iteration receive timeout.
    ///
    /// `timeout_ticks` is the maximum scheduler ticks to wait (one tick ≈ 10 ms).
    /// When that much time passes without an event the handler is called with
    /// [`AppEvent::Timeout`]; heartbeat refreshes in between do not shorten it.
    pub fn run_with_timeout(
        &mut self,
        timeout_ticks: u64,
        mut handler: impl FnMut(&mut AppContext, AppEvent),
    ) -> ! {
        let mut remaining = timeout_ticks.max(1);
        loop {
            let wait = match self.heartbeat_ticks {
                0 => remaining,
                h => remaining.min((h / 2).max(1)),
            };
            match self.wait_once(Some(wait)) {
                Some(Some(tid)) => {
                    remaining = timeout_ticks.max(1);
                    let event = Self::parse_event_owned(tid, self.payload());
                    handler(self, event);
                }
                Some(None) => {
                    remaining -= wait;
                    if remaining == 0 {
                        remaining = timeout_ticks.max(1);
                        handler(self, AppEvent::Timeout);
                    }
                }
                None => syscall::sys_yield(),
            }
        }
    }

    /// Answer the request `tid` sent, without parking. The request this loop
    /// received from `tid` is answered through its captured operation, even
    /// after the handler did other IPC; any other `tid` goes through
    /// [`crate::ipc::reply_to`].
    ///
    /// # Errors
    /// `IO` when the reply could not be delivered.
    pub fn reply(&self, tid: usize, data: &[u8]) -> ViResult<()> {
        match self.request {
            Some(request) if request.sender() == tid => request.reply(data),
            _ => crate::ipc::reply_to(tid, data),
        }
        .map_err(|_| ViError::IO)
    }

    /// Queue an App SDK envelope message for `tid`, without parking.
    ///
    /// Prepends `[APP_MSG_MAGIC, 0x00]` so the receiver's `AppContext` decodes it
    /// as [`AppEvent::Message`].
    ///
    /// # Errors
    /// `InvalidArgument` when the payload does not fit, `IO` when `tid` is gone
    /// or its mailbox is full.
    pub fn send_msg(&self, tid: usize, payload: &[u8]) -> ViResult<()> {
        let mut buf = [0u8; RECV_BUF_SIZE];
        if payload.len() + 2 > RECV_BUF_SIZE {
            return Err(ViError::InvalidArgument);
        }
        buf[0] = APP_MSG_MAGIC;
        buf[1] = 0x00;
        buf[2..2 + payload.len()].copy_from_slice(payload);
        crate::ipc::post(tid, &buf[..2 + payload.len()]).map_err(|_| ViError::IO)
    }

    /// Resolve the live provider tid of a well-known service.
    pub fn lookup_service(&self, service_id: u16) -> Option<usize> {
        crate::service::provider_tid(service_id)
    }

    /// Register this cell as the keyboard/mouse focus recipient.
    ///
    /// Delegates to [`ostd::input::request_focus`].  Call once at startup
    /// after any compositor surface is ready.  Returns `true` when the input
    /// service grants focus; `false` on boot race (retry with a yield).
    pub fn request_input_focus(&self) -> bool {
        crate::input::request_focus()
    }

    /// Arm (or re-arm) the kernel watchdog heartbeat for this cell.
    ///
    /// The kernel terminates the cell when no `Heartbeat` syscall arrives within
    /// `ticks` scheduler ticks (1 tick ≈ 10 ms); other syscalls do not count. The
    /// event loops ([`Self::run`], [`Self::run_with_timeout`]) re-arm it on every
    /// iteration, so only a handler stuck for a whole interval is killed. Pass `0`
    /// to disable. Called automatically by [`CellRuntime::run`].
    pub fn arm_heartbeat(&mut self, ticks: u64) {
        self.heartbeat_ticks = ticks;
        syscall::sys_heartbeat(ticks);
    }

    /// Run the IPC event loop, firing [`AppEvent::Init`] exactly once before
    /// the first `sys_recv`, then calling `handler` for every subsequent event.
    ///
    /// Prefer this over [`run`][Self::run] for cells that need startup work.
    pub fn run_with_lifecycle(&mut self, mut handler: impl FnMut(&mut AppContext, AppEvent)) -> ! {
        handler(self, AppEvent::Init);
        self.run(handler)
    }

    // ── Service client accessors ──────────────────────────────────────────────

    /// Lazy accessor for the VFS service client.
    ///
    /// Initializes on first call; reuses the cached `VfsRef` on subsequent calls.
    /// Note: returns `&mut VfsClient` which borrows `self` — store the result in a
    /// local to call other accessors in the same handler invocation.
    pub fn vfs(&mut self) -> &mut crate::clients::VfsClient {
        self.vfs_client
            .get_or_insert_with(crate::clients::VfsClient::new)
    }

    /// Lazy accessor for the network service client.
    pub fn net(&mut self) -> &mut crate::clients::NetClient {
        self.net_client
            .get_or_insert_with(crate::clients::NetClient::new)
    }

    /// Lazy accessor for the input service client.
    pub fn input(&mut self) -> &mut crate::clients::InputClient {
        self.input_client
            .get_or_insert_with(crate::clients::InputClient::new)
    }

    // ── Private ──────────────────────────────────────────────────────────────

    /// Parse `buf` into an owned `AppEvent`, copying the payload into a `Vec`.
    ///
    /// Owning the data eliminates the lifetime coupling between the event and the
    /// receive buffer, so the caller can pass `&mut self` to the handler freely.
    fn parse_event_owned(sender_tid: usize, buf: &[u8]) -> AppEvent {
        if buf.len() >= 2 && buf[0] == APP_MSG_MAGIC {
            match buf[1] {
                0xFF => {
                    // Structured shutdown: byte 2 carries the reason (if present).
                    if buf.len() >= 3 {
                        let reason = Self::parse_shutdown_reason(buf);
                        AppEvent::ShutdownWith { reason }
                    } else {
                        AppEvent::Shutdown
                    }
                }
                // Hot-swap Step 2: kernel asks this (old) cell to serialize state.
                // Envelope: [0xAC, 0xF0, swap_id_le8 (8 bytes)]
                0xF0 if buf.len() >= 10 => {
                    let mut id_bytes = [0u8; 8];
                    id_bytes.copy_from_slice(&buf[2..10]);
                    let swap_id = u64::from_le_bytes(id_bytes);
                    AppEvent::Snapshot { swap_id }
                }
                // Hot-swap Step 4: kernel asks this (new) cell to restore state.
                // Envelope: [0xAC, 0xF1, key (64 bytes)]
                0xF1 if buf.len() >= 66 => {
                    let mut key = [0u8; 64];
                    key.copy_from_slice(&buf[2..66]);
                    AppEvent::Restore { key }
                }
                // Runtime capability revocation.
                // Envelope: [0xAC, 0xF2, mask_le4 (4 bytes)]
                0xF2 if buf.len() >= 6 => {
                    let mut mask_bytes = [0u8; 4];
                    mask_bytes.copy_from_slice(&buf[2..6]);
                    AppEvent::CapRevoked {
                        mask: u32::from_le_bytes(mask_bytes),
                    }
                }
                _ => AppEvent::Message {
                    sender_tid,
                    data: buf[2..].to_vec(),
                },
            }
        } else if buf.len() >= 2 && buf[0] == api::input::INPUT_EVENT_OPCODE {
            if let Some(ev) = crate::input::parse_frame(buf) {
                return AppEvent::Input(ev);
            }
            AppEvent::RawMessage {
                sender_tid,
                data: buf.to_vec(),
            }
        } else {
            AppEvent::RawMessage {
                sender_tid,
                data: buf.to_vec(),
            }
        }
    }

    /// Decode shutdown reason byte from a `[0xAC, 0xFF, reason]` envelope.
    ///
    /// Missing or unknown reason bytes default to [`ShutdownReason::Requested`].
    pub(crate) fn parse_shutdown_reason(buf: &[u8]) -> ShutdownReason {
        match buf.get(2) {
            Some(1) => ShutdownReason::Watchdog,
            Some(2) => ShutdownReason::ParentDied,
            _ => ShutdownReason::Requested,
        }
    }
}

/// Re-export `CellRuntime` into the `app` module namespace for convenience.
pub use crate::runtime::CellRuntime;

#[cfg(test)]
mod tests {
    use super::*;

    /// The wire contract the kernel's `send_cap_revoked_event` produces:
    /// `[0xAC, 0xF2, mask_le4]` (spec/17 §3).
    #[test]
    fn cap_revoked_envelope_decodes_to_the_revoked_mask() {
        let mut buf = [0u8; 6];
        buf[0] = APP_MSG_MAGIC;
        buf[1] = 0xF2;
        buf[2..6].copy_from_slice(&0x0000_0504u32.to_le_bytes());

        match AppContext::parse_event_owned(7, &buf) {
            AppEvent::CapRevoked { mask } => assert_eq!(mask, 0x0000_0504),
            other => panic!("expected CapRevoked, got {other:?}"),
        }
    }

    /// A truncated `0xF2` envelope is not an event: it falls through to a
    /// Message rather than reporting a mask it never received.
    #[test]
    fn truncated_cap_revoked_envelope_is_not_decoded() {
        let buf = [APP_MSG_MAGIC, 0xF2, 0x04];
        assert!(matches!(
            AppContext::parse_event_owned(7, &buf),
            AppEvent::Message { .. }
        ));
    }
}

impl Default for AppContext {
    fn default() -> Self {
        Self::new()
    }
}
