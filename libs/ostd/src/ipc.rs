//! Typed request/reply IPC helpers — the compliant path for talking to a
//! service cell (Spec 17 — Cell IPC Wire Contract).
//!
//! Prefer [`service_call`] / [`service_call_typed`] over a hand-rolled
//! `sys_send` + `sys_recv(0)`: they recv **masked to the service tid** (Spec 17
//! §2), so a queued input key event can never be mistaken for the reply, and
//! they surface every failure as a typed [`IpcError`] instead of a silent empty
//! result (Spec 17 §7).

#![allow(unsafe_code)]

use crate::syscall::{sys_recv, sys_send, sys_try_recv, SyscallResult};
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};
use serde::{Deserialize, Serialize};

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
