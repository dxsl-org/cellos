//! Typed request/reply IPC helpers — the compliant path for talking to a
//! service cell (Spec 17 — Cell IPC Wire Contract).
//!
//! Prefer [`service_call`] / [`service_call_typed`] over a hand-rolled
//! `sys_send` + `sys_recv`: RPC uses a kernel-owned bounded operation and takes
//! only that operation's reply, including its actual byte length. Raw one-way
//! messages and [`recv_from`] retain the mailbox interface.

#![allow(unsafe_code)]

use crate::syscall::{sys_get_scheduler_ticks, sys_recv, sys_try_recv, sys_yield, SyscallResult};
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};
use serde::{Deserialize, Serialize};

/// Why a [`service_call`] did not complete. Never silently swallowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpcError {
    /// The request did not fit `send_buf` (postcard encode failed).
    Encode,
    /// Request submission was rejected, or a fastpath transport failed.
    Send,
    /// A raw receive or operation wait syscall returned an error.
    Recv,
    /// A message arrived, but from a cell other than the service — never
    /// treated as the reply (Spec 17 §7 silent-wrong-sender guard).
    WrongSender,
    /// The reply bytes did not decode into the expected type.
    Decode,
    /// Operation token is stale, foreign, cancelled, or otherwise invalid.
    InvalidOperation,
    /// Reply output buffer is too short. Low-level `take` retains the terminal;
    /// synchronous service calls drain their owned slot before returning this error.
    BufferTooSmall,
    /// Admission was busy; no request was accepted.
    Busy,
    /// The provider is gone. An accepted request may already have had side effects.
    /// This is not permission to retry automatically.
    PeerGone,
    /// The deadline elapsed before dispatch; this operation cannot execute later.
    PreDispatchTimeout,
    /// The request was dispatched but no reply settled it before expiry/cancellation.
    /// It may still execute; reconcile application state rather than retrying.
    Indeterminate,
    /// The operation was cancelled before dispatch.
    Cancelled,
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

/// Sleep until any result is terminal, or a finite scheduler-tick timeout
/// expires (one tick is 10 ms). Zero waits without a caller deadline; accepted
/// operations still have their separate kernel-enforced expiry. The kernel
/// rechecks results atomically with parking.
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

// ─── Nonblocking operation handles ───────────────────────────────────────────

/// One submitted bounded call: the kernel owns the request bytes and the reply slot,
/// so the caller may reuse its request buffer immediately after submission.
/// A terminal result retains its slot until taken.
///
/// Immediate servers may reply with plain `Send` to the currently served caller.
/// Deferred or multiple-outstanding servers must capture [`current`] before their
/// next receive and answer with [`reply`] using that exact token.
///
/// Several calls may be outstanding: submit each, then drain with [`wait`] and
/// [`try_take`][PendingCall::try_take]. Synchronous [`service_call`] uses this same
/// lifecycle. No accepted request is retried automatically.
///
/// Dropping the handle does not release its slot or roll back execution. Take the
/// result, or cancel and then take it. Post-dispatch cancellation is indeterminate.
pub struct PendingCall {
    operation: IpcOpId,
}

/// A settled operation: what happened, and how many reply bytes the kernel wrote into
/// the caller's buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Completion {
    /// The terminal kind, mapped to caller-visible outcomes by Spec 20 §2.4/§2.6.
    pub terminal: IpcTerminal,
    /// Reply bytes written into the caller's buffer; `0` for anything that is not
    /// [`IpcTerminal::Reply`].
    pub len: usize,
}

impl Completion {
    /// The response is known, or dispatch was prevented. A known reply can still
    /// contain application failure; this is not a blanket permission to retry.
    pub fn is_definite(&self) -> bool {
        matches!(
            self.terminal,
            IpcTerminal::Reply
                | IpcTerminal::PreDispatchTimeout
                | IpcTerminal::Cancelled
        )
    }

    /// Execution effects are unresolved. PeerGone may follow dispatch, including
    /// work delegated to a sibling service task. Never retry blindly; reconcile
    /// by request identity and application policy.
    pub fn is_uncertain(&self) -> bool {
        matches!(self.terminal, IpcTerminal::Indeterminate | IpcTerminal::PeerGone)
    }
}

impl PendingCall {
    /// Submit `request` without waiting for the peer to receive it.
    ///
    /// # Errors
    /// [`IpcSubmitError::Busy`] when nothing was accepted (no work queued — do not wait
    /// on it), [`IpcSubmitError::PeerGone`] when the peer is already gone, and
    /// [`IpcSubmitError::InvalidRequest`] for a malformed or oversized request.
    pub fn submit(peer_tid: usize, request: &[u8]) -> Result<Self, IpcSubmitError> {
        submit(peer_tid, request).map(|operation| Self { operation })
    }

    /// The exact operation id, for correlating a terminal with the request it belongs to.
    pub const fn operation(&self) -> IpcOpId {
        self.operation
    }

    /// Take the terminal result if it has arrived; `Ok(None)` while it is still
    /// outstanding. An undersized `reply` never consumes the result.
    pub fn try_take(&self, reply: &mut [u8]) -> Result<Option<Completion>, IpcError> {
        match take(self.operation, reply)? {
            IpcTakeResult::Pending => Ok(None),
            IpcTakeResult::Terminal { status, len } => Ok(Some(Completion {
                terminal: status,
                len,
            })),
        }
    }

    /// Wait, timer-bounded, for this call's terminal.
    ///
    /// `Ok(None)` means the budget ran out with the operation still outstanding — it is
    /// neither lost nor failed, so do not read it as an outcome; keep waiting, or decide
    /// per the contract that the caller's own deadline has passed.
    /// Each timed round is measured in scheduler ticks. A retained terminal for
    /// another call is left untouched and yields CPU rather than consuming a round.
    pub fn wait_and_take(
        &self,
        reply: &mut [u8],
        ticks_per_round: u64,
        rounds: usize,
    ) -> Result<Option<Completion>, IpcError> {
        for _ in 0..rounds.max(1) {
            let started = sys_get_scheduler_ticks().ok_or(IpcError::InvalidOperation)?;
            loop {
                if let Some(completion) = self.try_take(reply)? {
                    return Ok(Some(completion));
                }
                let ticks = if ticks_per_round == 0 {
                    0
                } else {
                    let ticks = remaining(started, ticks_per_round)?;
                    if ticks == 0 { break; }
                    ticks
                };
                match crate::syscall::sys_ipc_wait(ticks) {
                    0 => break,
                    // IpcWait sees every owned terminal, including results the
                    // caller deliberately retains. Do not spend a timed round
                    // on an unrelated terminal or starve this call's provider.
                    1 => {
                        if let Some(completion) = self.try_take(reply)? {
                            return Ok(Some(completion));
                        }
                        sys_yield();
                    }
                    _ => return Err(IpcError::Recv),
                }
            }
        }
        self.try_take(reply)
    }

    /// Abandon this operation. Pre-dispatch it becomes
    /// [`IpcTerminal::Cancelled`]; afterwards [`IpcTerminal::Indeterminate`], because a
    /// dispatched request is never rolled back. `take` the terminal afterwards to
    /// release the slot.
    pub fn cancel(&self) -> Result<(), IpcError> {
        cancel(self.operation)
    }
}

/// Remaining caller budget in scheduler ticks, not the kernel operation's expiry.
fn remaining(started: u64, budget: u64) -> Result<u64, IpcError> {
    let now = sys_get_scheduler_ticks().ok_or(IpcError::InvalidOperation)?;
    Ok(budget.saturating_sub(now.wrapping_sub(started)))
}

fn completion_len(completion: Completion) -> Result<usize, IpcError> {
    match completion.terminal {
        IpcTerminal::Reply => Ok(completion.len),
        IpcTerminal::PeerGone => Err(IpcError::PeerGone),
        IpcTerminal::PreDispatchTimeout => Err(IpcError::PreDispatchTimeout),
        IpcTerminal::Indeterminate => Err(IpcError::Indeterminate),
        IpcTerminal::Cancelled => Err(IpcError::Cancelled),
    }
}

/// Cancel pending execution and drain even a retained oversized reply. This scratch
/// is needed only on an error path: cancellation does not overwrite terminal Reply.
fn discard_call(call: &PendingCall) {
    let _ = call.cancel();
    let mut scratch = [0u8; api::ipc::IPC_BUF_SIZE];
    let _ = call.try_take(&mut scratch);
}

fn take_call(call: &PendingCall, reply: &mut [u8]) -> Result<Option<Completion>, IpcError> {
    match call.try_take(reply) {
        Ok(completion) => Ok(completion),
        Err(error) => {
            discard_call(call);
            Err(error)
        }
    }
}

fn exchange(
    service_tid: usize,
    request: &[u8],
    reply: &mut [u8],
    timeout_ticks: Option<u64>,
) -> Result<usize, IpcError> {
    let started = match timeout_ticks {
        Some(_) => sys_get_scheduler_ticks().ok_or(IpcError::InvalidOperation)?,
        None => 0,
    };
    let call = loop {
        if let Some(budget) = timeout_ticks {
            if remaining(started, budget)? == 0 {
                return Err(IpcError::PreDispatchTimeout);
            }
        }
        match PendingCall::submit(service_tid, request) {
            Ok(call) => break call,
            Err(IpcSubmitError::Busy) if timeout_ticks.is_some() => sys_yield(),
            Err(IpcSubmitError::Busy) => return Err(IpcError::Busy),
            Err(IpcSubmitError::PeerGone) => return Err(IpcError::PeerGone),
            Err(IpcSubmitError::InvalidRequest) => return Err(IpcError::Send),
        }
    };
    let mut other_terminal = false;
    loop {
        if let Some(completion) = take_call(&call, reply)? {
            return completion_len(completion);
        }
        let ticks = match timeout_ticks {
            Some(budget) => match remaining(started, budget) {
                Ok(0) => {
                    if let Err(error) = call.cancel() {
                        discard_call(&call);
                        return Err(error);
                    }
                    // A concurrent reply wins over cancellation and is still taken.
                    let completion = take_call(&call, reply)?
                        .ok_or(IpcError::InvalidOperation);
                    return match completion {
                        Ok(Completion { terminal: IpcTerminal::Cancelled, .. }) => {
                            Err(IpcError::PreDispatchTimeout)
                        }
                        Ok(completion) => completion_len(completion),
                        Err(error) => {
                            discard_call(&call);
                            Err(error)
                        }
                    };
                }
                Ok(ticks) => ticks,
                Err(error) => {
                    discard_call(&call);
                    return Err(error);
                }
            },
            None => 0,
        };
        if other_terminal {
            // IpcWait observes any terminal. Do not spin on a different owned slot
            // while starving the provider of this exact operation.
            sys_yield();
            other_terminal = false;
            continue;
        }
        match crate::syscall::sys_ipc_wait(ticks) {
            0 => {}
            1 => other_terminal = true,
            _ => {
                discard_call(&call);
                return Err(IpcError::Recv);
            }
        }
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

/// Receive a raw mailbox message from one peer. A masked `Recv` can still deliver
/// a queued NotifyOnExit record for another task; never decode it as the peer's
/// message. This interface cannot report the payload length; RPC uses exact operations.
pub fn recv_from<'r>(peer_tid: usize, recv_buf: &'r mut [u8]) -> Result<&'r [u8], IpcError> {
    reply_sender(peer_tid, sys_recv(peer_tid, recv_buf))?;
    Ok(recv_buf)
}

/// One kernel-bounded request/reply exchange with `service_tid`.
///
/// Submission never blocks in `Send`; Busy means no work was accepted. Once
/// accepted, wait for and take only this operation's terminal result. Provider death
/// and kernel expiry return errors even if the provider never replies. No accepted
/// request is retried. The returned slice covers exactly the reply bytes.
pub fn service_call<'r, Req: Serialize>(
    service_tid: usize,
    req: &Req,
    send_buf: &mut [u8],
    recv_buf: &'r mut [u8],
) -> Result<&'r [u8], IpcError> {
    let encoded = api::ipc::encode(req, send_buf).map_err(|_| IpcError::Encode)?;
    let len = exchange(service_tid, encoded, recv_buf, None)?;
    Ok(&recv_buf[..len])
}

#[cfg(test)]
mod tests {
    use super::{completion_len, reply_sender, Completion, IpcError, IpcTerminal};
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

    #[test]
    fn exact_completion_preserves_reply_length_and_terminal_outcomes() {
        assert_eq!(
            completion_len(Completion { terminal: IpcTerminal::Reply, len: 3 }),
            Ok(3)
        );
        for (terminal, error) in [
            (IpcTerminal::PeerGone, IpcError::PeerGone),
            (IpcTerminal::PreDispatchTimeout, IpcError::PreDispatchTimeout),
            (IpcTerminal::Indeterminate, IpcError::Indeterminate),
            (IpcTerminal::Cancelled, IpcError::Cancelled),
        ] {
            assert_eq!(completion_len(Completion { terminal, len: 0 }), Err(error));
        }
        let gone = Completion { terminal: IpcTerminal::PeerGone, len: 0 };
        assert!(gone.is_uncertain(), "provider death does not prove no side effects");
        assert!(!gone.is_definite());
        let cancelled = Completion { terminal: IpcTerminal::Cancelled, len: 0 };
        assert!(cancelled.is_definite(), "pre-dispatch cancellation prevents execution");
        assert!(!cancelled.is_uncertain());
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
/// Busy admission is retried only before acceptance. Once accepted, only this exact
/// operation is awaited; expiry cancels and takes its slot. Pre-dispatch expiry
/// returns [`IpcError::PreDispatchTimeout`]; after dispatch it returns
/// [`IpcError::Indeterminate`]. Late replies cannot be consumed by a subsequent call.
/// The caller's scheduler-tick budget is independent of the kernel's operation expiry.
pub fn service_call_bounded<'r, Req: Serialize>(
    service_tid: usize,
    req: &Req,
    send_buf: &mut [u8],
    recv_buf: &'r mut [u8],
    timeout_ticks: u64,
) -> Result<&'r [u8], IpcError> {
    let encoded = api::ipc::encode(req, send_buf).map_err(|_| IpcError::Encode)?;
    let len = exchange(service_tid, encoded, recv_buf, Some(timeout_ticks))?;
    Ok(&recv_buf[..len])
}

/// [`service_call_bounded`] that decodes the reply into `Resp`.
///
/// Uses the same end-to-end scheduler-tick budget and exact-operation cleanup as
/// [`service_call_bounded`]. A malformed reply returns [`IpcError::Decode`] after
/// its terminal slot has already been released.
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

/// Await a raw mailbox message on `mask` (0 = wildcard). For request/reply use
/// [`service_call`] instead, so provider death and late replies have exact outcomes.
pub fn recv_async(mask: usize, buf: &mut [u8]) -> AsyncRecv<'_> {
    AsyncRecv { mask, buf }
}
