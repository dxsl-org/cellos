// SPDX-License-Identifier: MPL-2.0

//! Actor runtime: a typed mailbox loop plus a supervisor tree.
//!
//! An actor is a cell (or a thread inside one cell) whose main loop receives the
//! App SDK envelope, decodes typed postcard messages, and answers them. The
//! library exists so an application can declare how it wants to be restarted
//! instead of editing `/bin/init`; the semantics it enforces are the ones the
//! project already proved there (Spec 12 §4.3).
//!
//! # Wire discipline (Spec 17)
//!
//! - Typed actor traffic rides the existing App SDK envelope
//!   ([`wire::proto::app_envelope`]) with kind `KIND_MESSAGE`, so
//!   [`Actor`] implementations keep receiving `Shutdown`, `CapRevoked`, and
//!   hot-swap events on the same mailbox. No new byte-0 value
//!   is claimed (§3).
//! - Requests and replies are **postcard**-encoded (§4) and sized to leave the
//!   2-byte envelope headroom (§5).
//! - Sends and replies never park: they queue in the peer's mailbox, and the
//!   caller takes the reply later. Pairs inside one actor use
//!   [`ActorCtx::call`], which recvs **masked to the peer tid** (§2) — never a wildcard that could swallow a keystroke or a child's
//!   exit notification.
//! - Nothing is dropped silently (§7): an undecodable message is logged with its
//!   sender and length.
//!
//! # Not in this library (by design, ADR-0021)
//!
//! There is no fire-and-forget send and no per-actor mailbox: the kernel mailbox
//! stays bounded and backpressured, and one tid has exactly one recv consumer
//! (§10.6). Actors are single-threaded event loops until B1's reactor lands.

pub mod supervisor;

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::app::{AppContext, AppEvent};
use crate::syscall::SyscallResult;
use crate::{syscall, ViError, ViResult};
use wire::proto::app_envelope::{HEADER_LEN, KIND_MESSAGE, MAGIC};

pub use supervisor::{Backoff, Child, ChildSpec, Decision, Policy, Strategy, Tree};

/// Log a line through the real console API (`ostd::io::println`).
///
/// Cell logging in this tree is `ostd::io::println(&format!(..))` — there is no
/// live `println!` macro (`libs/ostd/src/console.rs` is not declared as a module
/// and no cell uses it).
macro_rules! log {
    ($($arg:tt)*) => {
        crate::io::println(&alloc::format!($($arg)*))
    };
}

pub(crate) use log;

/// Scheduler ticks between two deadline checks (5 ticks ≈ 50 ms at 10 ms/tick).
///
/// A backoff timer therefore fires within this window of its deadline, which is
/// 1/20th of the 1 s restart bound B0 witnesses.
pub const ACTOR_TICK_TICKS: u64 = 5;

/// A typed message handler with a lifecycle.
///
/// Implementors are driven by [`run`], which owns the mailbox loop.
pub trait Actor: Sized {
    /// Typed application message carried in the `MAGIC, KIND_MESSAGE` envelope.
    type Msg: Serialize + DeserializeOwned;

    /// Runs once before the first receive. Register services, spawn children,
    /// request input focus here.
    fn on_start(&mut self, _ctx: &mut ActorCtx<'_>) {}

    /// Handle one decoded application message.
    fn on_message(&mut self, ctx: &mut ActorCtx<'_>, from: usize, msg: Self::Msg);

    /// Handle envelope traffic the actor has no typed arm for: kernel events,
    /// raw senders, and — for supervisors — the resume of a watched child.
    ///
    /// The default logs loudly rather than ignoring the event, because a silently
    /// dropped message is exactly the failure mode Spec 17 §7 prohibits.
    fn on_event(&mut self, _ctx: &mut ActorCtx<'_>, ev: AppEvent) {
        log!("[actor] unhandled event: {ev:?}");
    }

    /// Runs when no message arrived within [`ACTOR_TICK_TICKS`]. Use it for
    /// backoff timers and scripted progress.
    fn on_tick(&mut self, _ctx: &mut ActorCtx<'_>) {}

    /// Runs on a kernel-requested shutdown; never returns.
    fn on_shutdown(&mut self, ctx: &mut ActorCtx<'_>) -> ! {
        ctx.exit(0)
    }
}

/// Handle for talking to the rest of the system from inside an [`Actor`].
pub struct ActorCtx<'a> {
    app: &'a mut AppContext,
}

impl<'a> ActorCtx<'a> {
    fn new(app: &'a mut AppContext) -> Self {
        Self { app }
    }

    /// Monotonic scheduler ticks (10 ms slices) — the same clock the kernel uses
    /// for `RecvTimeout` deadlines.
    ///
    /// This is `GetTime` op 4, *not* op 0: op 0 returns the raw architected
    /// counter (10 MHz mtime on QEMU RV64), whose units are not ticks, and a
    /// budget or window compared against it silently never engages.
    ///
    /// The cell must declare `GetTime`; `0` means the clock was unavailable, and
    /// callers must treat that as "no time has passed" rather than as a stall.
    pub fn now_ticks(&self) -> u64 {
        syscall::sys_get_scheduler_ticks().unwrap_or(0)
    }

    /// Encode `msg` in the App SDK envelope into `frame`; returns its length.
    ///
    /// The payload must fit the frame *after* the 2-byte envelope (Spec 17 §5).
    fn frame<T: Serialize>(msg: &T, frame: &mut [u8; wire::ipc::IPC_BUF_SIZE]) -> ViResult<usize> {
        let mut scratch = [0u8; wire::ipc::IPC_BUF_SIZE];
        let payload_len = wire::ipc::encode(msg, &mut scratch)
            .map_err(|_| ViError::InvalidArgument)?
            .len();
        let total = payload_len + HEADER_LEN;
        if total > frame.len() {
            return Err(ViError::InvalidArgument);
        }
        frame[0] = MAGIC;
        frame[1] = KIND_MESSAGE;
        frame[HEADER_LEN..total].copy_from_slice(&scratch[..payload_len]);
        Ok(total)
    }

    /// Queue a typed message in the App SDK envelope for `to`, without parking.
    ///
    /// # Errors
    /// `InvalidArgument` when the message does not fit, `IO` when `to` is gone
    /// or its mailbox is full.
    pub fn send_msg<T: Serialize>(&mut self, to: usize, msg: &T) -> ViResult<()> {
        let mut frame = [0u8; wire::ipc::IPC_BUF_SIZE];
        let len = Self::frame(msg, &mut frame)?;
        crate::ipc::post(to, &frame[..len]).map_err(|_| ViError::IO)
    }

    /// Answer the request `to` sent, without parking (see
    /// [`crate::ipc::reply_to`]); the caller need not be receiving yet.
    ///
    /// # Errors
    /// As [`ActorCtx::send_msg`].
    pub fn reply<T: Serialize>(&mut self, to: usize, msg: &T) -> ViResult<()> {
        let mut frame = [0u8; wire::ipc::IPC_BUF_SIZE];
        let len = Self::frame(msg, &mut frame)?;
        self.app.reply(to, &frame[..len])
    }

    /// Typed request/reply exchange with `to`.
    ///
    /// The receive is masked to `to` (Spec 17 §2) and expects the peer's reply in
    /// the same envelope, so a queued input event or a child exit can never be
    /// mistaken for the answer.
    pub fn call<Req, Resp>(&mut self, to: usize, req: &Req) -> ViResult<Resp>
    where
        Req: Serialize,
        Resp: DeserializeOwned,
    {
        self.send_msg(to, req)?;
        let mut buf = [0u8; wire::ipc::IPC_BUF_SIZE];
        match syscall::sys_recv(to, &mut buf) {
            SyscallResult::Ok(sender) if sender == to => {}
            SyscallResult::Ok(_) => return Err(ViError::IO),
            SyscallResult::Err(_) => return Err(ViError::IO),
        }
        if buf[0] != MAGIC || buf[1] != KIND_MESSAGE {
            return Err(ViError::IO);
        }
        wire::ipc::decode::<Resp>(&buf[HEADER_LEN..]).map_err(|_| ViError::IO)
    }

    /// Spawn a cell from a path. Returns its tid.
    ///
    /// Requires `SpawnCap`; the kernel authorizes the exact `(caller, route, path)`
    /// edge (`SpawnFromPath`, op 12).
    pub fn spawn(&mut self, path: &str) -> ViResult<usize> {
        match syscall::sys_spawn_from_path(path) {
            SyscallResult::Ok(tid) if tid > 0 => Ok(tid),
            SyscallResult::Ok(_) => Err(ViError::IO),
            SyscallResult::Err(_) => Err(ViError::PermissionDenied),
        }
    }

    /// Ask the kernel to deliver this child's exit reason to our mailbox.
    ///
    /// One-shot and `SpawnCap`-gated: after a delivery the supervisor must arm it
    /// again for the respawned child (`NotifyOnExit`, op 204).
    pub fn watch(&mut self, tid: usize) -> ViResult<()> {
        match syscall::sys_notify_on_exit(tid) {
            SyscallResult::Ok(_) => Ok(()),
            SyscallResult::Err(_) => Err(ViError::PermissionDenied),
        }
    }

    /// Terminate `tid` with the fault sentinel reason (`ForceExit`, op 61).
    ///
    /// Requires `SpawnCap`; self-kill and critical driver cells are refused.
    pub fn force_exit(&mut self, tid: usize) -> ViResult<()> {
        match syscall::sys_force_exit(tid) {
            SyscallResult::Ok(_) => Ok(()),
            SyscallResult::Err(_) => Err(ViError::PermissionDenied),
        }
    }

    /// Resolve a well-known service's live provider tid.
    pub fn lookup(&mut self, service_id: u16) -> Option<usize> {
        self.app.lookup_service(service_id)
    }

    /// Terminate this actor with `code`; never returns.
    pub fn exit(&mut self, code: usize) -> ! {
        syscall::sys_exit(code)
    }
}

/// Decode the kernel's exit-reason payload delivered with a watch resume.
///
/// `NotifyOnExit` delivers the reason as 8 native-endian bytes in the receive
/// buffer, followed by whatever stale bytes the buffer held; the length is not
/// part of the contract, so a short buffer is not an exit notification.
pub fn exit_reason(data: &[u8]) -> Option<u64> {
    let bytes: [u8; 8] = data.get(..8)?.try_into().ok()?;
    Some(u64::from_ne_bytes(bytes))
}

/// Run an actor's mailbox loop; never returns.
///
/// Calls [`Actor::on_start`] once, then hands every event to the actor. Deadline
/// ticks arrive every [`ACTOR_TICK_TICKS`], which is what drives supervisor
/// backoff timers.
pub fn run<A: Actor>(mut actor: A) -> ! {
    let mut app = AppContext::new();
    {
        let mut ctx = ActorCtx::new(&mut app);
        actor.on_start(&mut ctx);
    }
    app.run_with_timeout(ACTOR_TICK_TICKS, move |app, ev| {
        let mut ctx = ActorCtx::new(app);
        match ev {
            AppEvent::Message { sender_tid, data } => match wire::ipc::decode::<A::Msg>(&data) {
                Ok(msg) => actor.on_message(&mut ctx, sender_tid, msg),
                Err(_) => log!(
                    "[actor] undecodable message from tid={sender_tid} (len={}): dropped loudly",
                    data.len()
                ),
            },
            AppEvent::Timeout => actor.on_tick(&mut ctx),
            AppEvent::Init => actor.on_start(&mut ctx),
            AppEvent::Shutdown | AppEvent::ShutdownWith { .. } => actor.on_shutdown(&mut ctx),
            other => actor.on_event(&mut ctx, other),
        }
    })
}
