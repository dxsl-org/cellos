//! IPC and timing helpers shared by the three recovery-demo Cells; each Cell
//! uses a subset.

#![allow(dead_code)]

use api::caller_identity::CallerIdentity;
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
}

impl Received {
    /// The attested principal, only when the trailer names this exact sender.
    pub fn principal(&self) -> Option<(u64, u64)> {
        let id = self.identity?;
        (id.sender_tid == self.sender as u64 && id.cell_id != 0 && id.generation != 0)
            .then_some((id.cell_id, id.generation))
    }
}

pub fn send(tid: usize, msg: Msg) -> bool {
    matches!(syscall::sys_send(tid, &msg.encode()), SyscallResult::Ok(_))
}

/// Blocking attested receive from `mask` (0 = anyone).
pub fn recv_attested(mask: usize) -> Option<Received> {
    let mut bytes = [0u8; RECV_LEN];
    match syscall::sys_recv_attested(mask, &mut bytes) {
        SyscallResult::Ok(sender) if sender != 0 && sender != usize::MAX => Some(Received {
            sender,
            msg: Msg::decode(&bytes),
            identity: CallerIdentity::from_recv_buf(&bytes),
            bytes,
        }),
        _ => None,
    }
}

/// Reply frame from exactly `tid`, or `None` on timeout or peer death (the
/// kernel completes a receive masked to a dead peer with a zero payload, which
/// is not a frame).
pub fn recv_from(tid: usize, timeout_ticks: u64) -> Option<Msg> {
    let mut bytes = [0u8; RECV_LEN];
    match syscall::sys_recv_timeout(tid, &mut bytes, timeout_ticks) {
        SyscallResult::Ok(sender) if sender == tid => Msg::decode(&bytes),
        _ => None,
    }
}

/// Like [`recv_from`] but returns the raw bytes too (multi-part replies).
pub fn recv_raw_from(tid: usize, timeout_ticks: u64, bytes: &mut [u8]) -> bool {
    matches!(syscall::sys_recv_timeout(tid, bytes, timeout_ticks), SyscallResult::Ok(sender) if sender == tid)
}

pub fn sleep(ticks: usize) {
    let _ = syscall::sys_set_timer(ticks);
}

pub fn lookup(service: u16) -> Option<usize> {
    syscall::sys_lookup_service(service).filter(|tid| *tid != 0)
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
