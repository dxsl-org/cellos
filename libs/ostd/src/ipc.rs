//! Typed request/reply IPC helpers — the compliant path for talking to a
//! service cell (Spec 17 — Cell IPC Wire Contract).
//!
//! Prefer [`service_call`] / [`service_call_typed`] over a hand-rolled
//! `sys_send` + `sys_recv(0)`: they recv **masked to the service tid** (Spec 17
//! §2), so a queued input key event can never be mistaken for the reply, and
//! they surface every failure as a typed [`IpcError`] instead of a silent empty
//! result (Spec 17 §7).

#![allow(unsafe_code)]

use crate::syscall::{
    sys_get_scheduler_ticks, sys_recv, sys_recv_timeout, sys_send, sys_try_recv, sys_try_send,
    sys_yield, SyscallResult,
};
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};
use serde::{Deserialize, Serialize};

mod deadline;

/// Why a [`service_call`] did not complete. Never silently swallowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpcError {
    /// The request did not fit `send_buf` (postcard encode failed).
    Encode,
    /// `sys_send` to the service failed (service gone / bad tid).
    Send,
    /// `sys_recv` returned an error.
    Recv,
    /// A message arrived, but from a cell other than the service — never
    /// treated as the reply (Spec 17 §7 silent-wrong-sender guard).
    WrongSender,
    /// The reply bytes did not decode into the expected type.
    Decode,
    /// Operation token is stale, foreign, cancelled, or otherwise invalid.
    InvalidOperation,
    /// Reply output buffer is too short; the terminal result was not consumed.
    BufferTooSmall,
}

/// Exact operation identity scoped to the caller's live task generation.
pub type IpcOpId = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpcSubmitError {
    Busy,
    PeerGone,
    InvalidRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpcTerminal {
    Reply,
    PeerGone,
    PreDispatchTimeout,
    Indeterminate,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpcTakeResult {
    Pending,
    Terminal { status: IpcTerminal, len: usize },
}

/// Copies `request` into kernel storage, without waiting for the peer to
/// receive or reply. Busy means no work was accepted; do not wait on this id.
pub fn submit(peer_tid: usize, request: &[u8]) -> Result<IpcOpId, IpcSubmitError> {
    match crate::syscall::sys_ipc_submit(peer_tid, request) {
        token if token > 0 => Ok(token as usize),
        -2 => Err(IpcSubmitError::Busy),
        -3 => Err(IpcSubmitError::PeerGone),
        _ => Err(IpcSubmitError::InvalidRequest),
    }
}

/// Atomically take the exact operation's retained terminal result. The reply
/// bytes are owned by the kernel until this succeeds; an undersized `reply`
/// never consumes the result. A terminal result releases the operation slot.
pub fn take(op: IpcOpId, reply: &mut [u8]) -> Result<IpcTakeResult, IpcError> {
    let mut status = [0u8; api::syscall::IPC_STATUS_LEN];
    match crate::syscall::sys_ipc_take(op, reply, &mut status) {
        0 => Ok(IpcTakeResult::Pending),
        1 => {
            let version = u32::from_le_bytes(status[..4].try_into().unwrap());
            let kind = u32::from_le_bytes(status[4..8].try_into().unwrap());
            let len = u32::from_le_bytes(status[8..12].try_into().unwrap()) as usize;
            let reserved = u32::from_le_bytes(status[12..16].try_into().unwrap());
            if version != api::syscall::IPC_STATUS_VERSION || reserved != 0 || len > reply.len() {
                return Err(IpcError::Decode);
            }
            let status = match kind {
                api::syscall::ipc_status::REPLY => IpcTerminal::Reply,
                api::syscall::ipc_status::PEER_GONE => IpcTerminal::PeerGone,
                api::syscall::ipc_status::PRE_DISPATCH_TIMEOUT => IpcTerminal::PreDispatchTimeout,
                api::syscall::ipc_status::INDETERMINATE => IpcTerminal::Indeterminate,
                api::syscall::ipc_status::CANCELLED => IpcTerminal::Cancelled,
                _ => return Err(IpcError::Decode),
            };
            Ok(IpcTakeResult::Terminal { status, len })
        }
        -5 => Err(IpcError::BufferTooSmall),
        _ => Err(IpcError::InvalidOperation),
    }
}

/// Sleep until any result is terminal, or a finite timeout expires. Zero
/// waits indefinitely; the kernel rechecks results atomically with parking.
pub fn wait(timeout_ticks: u64) -> bool {
    crate::syscall::sys_ipc_wait(timeout_ticks) == 1
}

/// Abandon one exact operation. A pre-dispatch request becomes Cancelled;
/// an already-dispatched request becomes Indeterminate (not rolled back).
/// Call `take` afterwards to release the retained terminal slot.
pub fn cancel(op: IpcOpId) -> Result<(), IpcError> {
    match crate::syscall::sys_ipc_cancel(op) {
        0 => Ok(()),
        _ => Err(IpcError::InvalidOperation),
    }
}

/// Token of the current async service request, if the last receive delivered
/// one. Save it before receiving another request for a deferred reply.
pub fn current() -> Option<IpcOpId> {
    match crate::syscall::sys_ipc_current() {
        token if token > 0 => Some(token as usize),
        _ => None,
    }
}

/// Reply to a deferred request bound to this exact provider incarnation.
/// Cancelled, duplicate, dead-client and foreign tokens are rejected.
pub fn reply(op: IpcOpId, bytes: &[u8]) -> Result<(), IpcError> {
    match crate::syscall::sys_ipc_reply(op, bytes) {
        0 => Ok(()),
        _ => Err(IpcError::InvalidOperation),
    }
}

/// A masked receive may still return an unrelated NotifyOnExit record.
#[inline]
fn reply_sender(peer_tid: usize, result: SyscallResult) -> Result<(), IpcError> {
    match result {
        SyscallResult::Ok(sender) if sender == peer_tid => Ok(()),
        SyscallResult::Ok(_) => Err(IpcError::WrongSender),
        SyscallResult::Err(_) => Err(IpcError::Recv),
    }
}

/// Receive a reply from one peer. A masked `Recv` can still deliver a queued
/// NotifyOnExit record for another task, returning that task's id and writing
/// its exit reason into the reply buffer. Never decode it as the peer's reply.
pub fn recv_from<'r>(peer_tid: usize, recv_buf: &'r mut [u8]) -> Result<&'r [u8], IpcError> {
    reply_sender(peer_tid, sys_recv(peer_tid, recv_buf))?;
    Ok(recv_buf)
}

/// One request/reply exchange with `service_tid`, recv **masked** to it.
///
/// `send_buf` encodes the request; `recv_buf` receives the reply and backs the
/// returned slice (caller-owned so the borrow outlives the call). The reply is
/// accepted only if it came from `service_tid` — a message from any other
/// sender (e.g. a queued input event, Spec 17 §2) is an [`IpcError::WrongSender`],
/// not a decode of the wrong bytes.
pub fn service_call<'r, Req: Serialize>(
    service_tid: usize,
    req: &Req,
    send_buf: &mut [u8],
    recv_buf: &'r mut [u8],
) -> Result<&'r [u8], IpcError> {
    let encoded = api::ipc::encode(req, send_buf).map_err(|_| IpcError::Encode)?;
    if let SyscallResult::Err(_) = sys_send(service_tid, encoded) {
        return Err(IpcError::Send);
    }
    // MASKED recv — Spec 17 §2. A queued death for another task can still
    // bypass the mask; recv_from checks the returned sender before decoding.
    recv_from(service_tid, recv_buf)
}

#[cfg(test)]
mod recv_tests {
    use super::{reply_sender, IpcError};
    use crate::syscall::{SyscallError, SyscallResult};

    #[test]
    fn masked_receive_rejects_unrelated_death_instead_of_decoding_reason() {
        assert_eq!(reply_sender(17, SyscallResult::Ok(31)), Err(IpcError::WrongSender));
        assert_eq!(
            reply_sender(17, SyscallResult::Err(SyscallError::TryAgain)),
            Err(IpcError::Recv)
        );
        assert_eq!(reply_sender(17, SyscallResult::Ok(17)), Ok(()));
    }
}

/// [`service_call`] that decodes the reply into `Resp`.
///
/// `Resp` borrows `recv_buf` for types with `&str`/`&[u8]` fields — consume it
/// before reusing the buffer.
pub fn service_call_typed<'r, Req, Resp>(
    service_tid: usize,
    req: &Req,
    send_buf: &mut [u8],
    recv_buf: &'r mut [u8],
) -> Result<Resp, IpcError>
where
    Req: Serialize,
    Resp: Deserialize<'r>,
{
    let raw = service_call(service_tid, req, send_buf, recv_buf)?;
    api::ipc::decode::<Resp>(raw).map_err(|_| IpcError::Decode)
}

/// One bounded request/reply exchange with `service_tid`.
///
/// `send_buf` is used to encode `req`; an encoding failure returns
/// [`IpcError::Encode`]. `recv_buf` receives the reply and backs the returned
/// slice. `timeout_ticks` is the end-to-end scheduler-tick budget shared by
/// request admission and the reply wait.
///
/// The encoded request is offered with nonblocking [`sys_try_send`] until the
/// deadline. A rejected admission yields before retrying, so a service waiting
/// on a nested dependency can return to wildcard `Recv`. Once accepted, the
/// request is never resent and only the remaining deadline is available to the
/// sender-masked receive. Admission expiry returns [`IpcError::Send`]; receive
/// expiry or syscall failure returns [`IpcError::Recv`]. After a receive error,
/// callers must poison that service generation because a late reply may arrive.
pub fn service_call_bounded<'r, Req: Serialize>(
    service_tid: usize,
    req: &Req,
    send_buf: &mut [u8],
    recv_buf: &'r mut [u8],
    timeout_ticks: u64,
) -> Result<&'r [u8], IpcError> {
    let encoded = api::ipc::encode(req, send_buf).map_err(|_| IpcError::Encode)?;
    let result = deadline::exchange_until_deadline(
        service_tid,
        timeout_ticks,
        || matches!(sys_try_send(service_tid, encoded), SyscallResult::Ok(0)),
        sys_get_scheduler_ticks,
        sys_yield,
        |remaining| match sys_recv_timeout(service_tid, recv_buf, remaining) {
            SyscallResult::Ok(sender) => Ok(sender),
            SyscallResult::Err(_) => Err(()),
        },
    );
    match result {
        Ok(()) => {
            let len = recv_buf.len();
            Ok(&recv_buf[..len])
        }
        Err(deadline::ExchangeError::Send) => Err(IpcError::Send),
        Err(deadline::ExchangeError::Recv) => Err(IpcError::Recv),
        Err(deadline::ExchangeError::WrongSender) => Err(IpcError::WrongSender),
    }
}

/// [`service_call_bounded`] that decodes the reply into `Resp`.
///
/// Send admission yields until the shared deadline, but the request is never
/// resent after delivery. Admission expiry returns [`IpcError::Send`]. The
/// sender-masked receive gets only the remaining budget; timeout and receive
/// errors return [`IpcError::Recv`], while malformed bytes return
/// [`IpcError::Decode`].
///
/// `send_buf` holds the encoded `req`. `recv_buf` receives the reply and is
/// borrowed by `Resp` when it contains `&str` or `&[u8]` fields, so consume the
/// response before reusing the buffer.
pub fn service_call_typed_bounded<'r, Req, Resp>(
    service_tid: usize,
    req: &Req,
    send_buf: &mut [u8],
    recv_buf: &'r mut [u8],
    timeout_ticks: u64,
) -> Result<Resp, IpcError>
where
    Req: Serialize,
    Resp: Deserialize<'r>,
{
    let raw = service_call_bounded(service_tid, req, send_buf, recv_buf, timeout_ticks)?;
    api::ipc::decode::<Resp>(raw).map_err(|_| IpcError::Decode)
}

/// [`service_call_bounded`] whose request admission **queues** instead of
/// rendezvousing.
///
/// [`service_call_bounded`] offers the request with nonblocking [`sys_try_send`],
/// which the kernel admits only while the target sits in `Recv` with a mask that
/// matches the caller. A service that idles in a completion wait — the Net Cell
/// sleeps on the `NET_RX` completion source — or that is simply running its own
/// loop is never in that state, so the offer is refused for the whole deadline
/// and the caller sees [`IpcError::Send`] against a perfectly healthy service.
///
/// This variant sends with [`sys_send`], which queues into the receiver's
/// mailbox and wakes a `Recv` or a `NET_RX` completion waiter, then waits for
/// the reply with the same bounded, sender-masked receive. Admission is
/// therefore unbounded: a service that never receives leaves the caller blocked
/// in the kernel's `Sending` state. Use it for a service that is known to drain
/// its mailbox on every turn (the Net Cell does, before it parks).
///
/// `timeout_ticks` bounds only the reply wait. Errors are as in
/// [`service_call_bounded`].
pub fn service_call_bounded_queued<'r, Req: Serialize>(
    service_tid: usize,
    req: &Req,
    send_buf: &mut [u8],
    recv_buf: &'r mut [u8],
    timeout_ticks: u64,
) -> Result<&'r [u8], IpcError> {
    /// Most foreign messages tolerated before the call gives up. The time base
    /// below bounds the wait; this bounds the loop for a kernel that does not
    /// provide one, so "keep waiting" can never become an unbounded spin.
    const MAX_FOREIGN_MESSAGES: u32 = 4;

    let encoded = api::ipc::encode(req, send_buf).map_err(|_| IpcError::Encode)?;
    if let SyscallResult::Err(_) = sys_send(service_tid, encoded) {
        return Err(IpcError::Send);
    }
    // A sender-masked receive can still return a *death notification*: the kernel
    // serves those regardless of the mask and documents that the caller must tell
    // them apart by the returned sender. Failing the whole call on the first one
    // is what cost the board's guest its traffic — the hypervisor's `L2Send` came
    // back `wrong sender`, so `transmit` returned false, the guest's TX descriptor
    // was never completed (`[hv-virtio-host] net-tx-complete` never printed) and
    // everything the guest sent died in its own TX queue. The foreign message is
    // consumed either way, so the wait names it once and continues under the
    // *same* deadline; `sys_get_scheduler_ticks` is the time base
    // `sys_recv_timeout` itself uses, and a kernel without it keeps the old
    // single-wait behaviour.
    let deadline = sys_get_scheduler_ticks().map(|now| now.saturating_add(timeout_ticks));
    let mut foreign = 0u32;
    loop {
        let remaining = match deadline {
            Some(deadline) => {
                let now = sys_get_scheduler_ticks().unwrap_or(0);
                if now >= deadline {
                    return Err(IpcError::Recv);
                }
                deadline - now
            }
            None => timeout_ticks,
        };
        match sys_recv_timeout(service_tid, recv_buf, remaining) {
            SyscallResult::Ok(sender) if sender == service_tid => {
                let len = recv_buf.len();
                return Ok(&recv_buf[..len]);
            }
            SyscallResult::Ok(sender) => {
                use core::sync::atomic::{AtomicBool, Ordering};
                static FIRST_FOREIGN: AtomicBool = AtomicBool::new(false);
                if !FIRST_FOREIGN.swap(true, Ordering::Relaxed) {
                    crate::io::print("[ipc] message from tid ");
                    crate::io::print_usize(sender);
                    crate::io::print(", expected ");
                    crate::io::print_usize(service_tid);
                    crate::io::println(" — consumed, still waiting for the reply");
                }
                foreign += 1;
                if deadline.is_none() || foreign > MAX_FOREIGN_MESSAGES {
                    return Err(IpcError::WrongSender);
                }
            }
            SyscallResult::Err(_) => return Err(IpcError::Recv),
        }
    }
}

/// [`service_call_bounded_queued`] that decodes the reply into `Resp`.
pub fn service_call_typed_bounded_queued<'r, Req, Resp>(
    service_tid: usize,
    req: &Req,
    send_buf: &mut [u8],
    recv_buf: &'r mut [u8],
    timeout_ticks: u64,
) -> Result<Resp, IpcError>
where
    Req: Serialize,
    Resp: Deserialize<'r>,
{
    let raw = service_call_bounded_queued(service_tid, req, send_buf, recv_buf, timeout_ticks)?;
    api::ipc::decode::<Resp>(raw).map_err(|_| IpcError::Decode)
}

/// Fastpath zero-trap RPC service call over an established SPSC ring channel endpoint.
///
/// Encodes `req` into `send_buf` via postcard, transmits it directly into the SPSC
/// ring buffer without trapping to kernel mode, spins briefly and yields if needed,
/// and receives the reply directly from the return ring buffer.
pub fn fastpath_call<'r, Req: Serialize>(
    endpoint: &crate::ring_channel::FastpathEndpoint<'_>,
    req: &Req,
    send_buf: &mut [u8],
    recv_buf: &'r mut [u8],
) -> Result<&'r [u8], IpcError> {
    let encoded = api::ipc::encode(req, send_buf).map_err(|_| IpcError::Encode)?;
    let len = endpoint
        .call(encoded, recv_buf)
        .map_err(|_| IpcError::Send)?;
    Ok(&recv_buf[..len])
}

/// Typed version of `fastpath_call` that deserializes the reply into `Resp`.
pub fn fastpath_call_typed<'r, Req, Resp>(
    endpoint: &crate::ring_channel::FastpathEndpoint<'_>,
    req: &Req,
    send_buf: &mut [u8],
    recv_buf: &'r mut [u8],
) -> Result<Resp, IpcError>
where
    Req: Serialize,
    Resp: Deserialize<'r>,
{
    let raw = fastpath_call(endpoint, req, send_buf, recv_buf)?;
    api::ipc::decode::<Resp>(raw).map_err(|_| IpcError::Decode)
}

// ── Async recv (naive-executor future) ────────────────────────────────────────

/// Future that waits for a message to arrive. Returns the sender id.
pub struct AsyncRecv<'a> {
    pub mask: usize,
    pub buf: &'a mut [u8],
}

impl<'a> Future for AsyncRecv<'a> {
    type Output = SyscallResult;

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        match sys_try_recv(self.mask, self.buf) {
            // No message yet — the naive executor yields and polls again.
            SyscallResult::Ok(0) => Poll::Pending,
            SyscallResult::Ok(id) => Poll::Ready(SyscallResult::Ok(id)),
            err => Poll::Ready(err),
        }
    }
}

/// Await a message on `mask` (0 = wildcard). Prefer a service tid for
/// request/reply — see Spec 17 §2.
pub fn recv_async(mask: usize, buf: &mut [u8]) -> AsyncRecv<'_> {
    AsyncRecv { mask, buf }
}
