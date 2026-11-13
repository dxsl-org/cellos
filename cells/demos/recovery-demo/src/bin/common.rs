//! IPC and timing helpers shared by the three recovery-demo Cells; each Cell
//! uses a subset.
//!
//! A request expecting an answer is one exact operation ([`call`],
//! [`call_raw`]); its answer goes back through the [`Incoming`] captured when
//! the request was received ([`reply`]). One-way messages are [`post`]ed.

#![allow(dead_code)]

use abi::caller_identity::CallerIdentity;
use ostd::ipc::Incoming;
use ostd::syscall::{self, SyscallResult};
use recovery_demo::wire::Msg;
use recovery_demo::RECV_LEN;

/// One received frame. `identity` is the kernel-written trailer; `None` means
/// the kernel attested no live sender (a death notification or a forgery).
pub struct Received {
    pub sender: usize,
    pub msg: Option<Msg>,
    pub identity: Option<CallerIdentity>,
    pub bytes: [u8; RECV_LEN],
    /// Where the answer to this frame goes; captured before any other receive.
    pub request: Incoming,
}

impl Received {
    /// The attested principal, only when the trailer names this exact sender.
    pub fn principal(&self) -> Option<(u64, u64)> {
        let id = self.identity?;
        (id.sender_tid == self.sender as u64 && id.cell_id != 0 && id.generation != 0)
            .then_some((id.cell_id, id.generation))
    }
}

/// Request `msg` from `tid` and take its one answer frame within `timeout_ticks`
/// (admission included). `None` on refusal, expiry, peer death or a malformed answer.
pub fn call(tid: usize, msg: Msg, timeout_ticks: u64) -> Option<Msg> {
    let mut bytes = [0u8; RECV_LEN];
    let reply = ostd::ipc::call_bytes(tid, &msg.encode(), &mut bytes, timeout_ticks).ok()?;
    Msg::decode(reply)
}

/// Like [`call`] but leaves the raw answer in `bytes` (frame plus trailing record).
pub fn call_raw(tid: usize, msg: Msg, timeout_ticks: u64, bytes: &mut [u8]) -> bool {
    ostd::ipc::call_bytes(tid, &msg.encode(), bytes, timeout_ticks).is_ok()
}

/// Answer the request `to` captured.
pub fn reply(to: &Incoming, msg: Msg) -> bool {
    reply_bytes(to, &msg.encode())
}

/// Answer the request `to` captured with raw bytes (a frame plus a trailing record).
pub fn reply_bytes(to: &Incoming, bytes: &[u8]) -> bool {
    to.reply(bytes).is_ok()
}

/// One-way message: nothing answers it.
pub fn post(tid: usize, msg: Msg) -> bool {
    ostd::ipc::post(tid, &msg.encode()).is_ok()
}

/// Blocking attested receive from `mask` (0 = anyone).
pub fn recv_attested(mask: usize) -> Option<Received> {
    let mut bytes = [0u8; RECV_LEN];
    match syscall::sys_recv_attested(mask, &mut bytes) {
        SyscallResult::Ok(sender) if sender != 0 && sender != usize::MAX => Some(Received {
            sender,
            request: Incoming::current(sender),
            msg: Msg::decode(&bytes),
            identity: CallerIdentity::from_recv_buf(&bytes),
            bytes,
        }),
        _ => None,
    }
}

/// Request frame from exactly `tid` with the [`Incoming`] that answers it, or
/// `None` on timeout or peer death (the kernel completes a receive masked to a
/// dead peer with a zero payload, which is not a frame).
pub fn recv_request_from(tid: usize, timeout_ticks: u64) -> Option<(Option<Msg>, Incoming)> {
    let mut bytes = [0u8; RECV_LEN];
    match syscall::sys_recv_timeout(tid, &mut bytes, timeout_ticks) {
        SyscallResult::Ok(sender) if sender == tid => {
            Some((Msg::decode(&bytes), Incoming::current(sender)))
        }
        _ => None,
    }
}

pub fn sleep(ticks: usize) {
    let _ = syscall::sys_set_timer(ticks);
}

pub fn lookup(service: u16) -> Option<usize> {
    ostd::service::provider_tid(service).filter(|tid| *tid != 0)
}

/// Poll `service` until it resolves, at most `tries` one-tick waits.
pub fn wait_lookup(service: u16, tries: usize) -> Option<usize> {
    for _ in 0..tries {
        if let Some(tid) = lookup(service) {
            return Some(tid);
        }
        sleep(1);
    }
    None
}
