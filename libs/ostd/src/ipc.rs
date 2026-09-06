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
    // MASKED recv — Spec 17 §2. Only the service's reply, never a keystroke.
    match sys_recv(service_tid, recv_buf) {
        SyscallResult::Ok(sender) if sender == service_tid => {
            let len = recv_buf.len();
            Ok(&recv_buf[..len])
        }
        SyscallResult::Ok(_) => Err(IpcError::WrongSender),
        SyscallResult::Err(_) => Err(IpcError::Recv),
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
