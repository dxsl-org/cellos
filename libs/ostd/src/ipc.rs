//! Typed request/reply IPC helpers — the compliant path for talking to a
//! service cell (Spec 17 — Cell IPC Wire Contract).
//!
//! Prefer [`service_call`] / [`service_call_typed`] over a hand-rolled
//! `sys_send` + `sys_recv`: RPC uses a kernel-owned bounded operation and takes
//! only that operation's reply, including its actual byte length. Raw one-way
//! messages and [`recv_from`] retain the mailbox interface.

#![allow(unsafe_code)]

use crate::syscall::{
    sys_get_scheduler_ticks, sys_recv, sys_try_recv, sys_wait_completion_detailed, sys_yield,
    SyscallResult, WaitCompletionResult,
};
use api::completion::source;
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

/// Sleep until an owned operation reports a terminal, or a finite
/// scheduler-tick timeout expires (one tick is 10 ms). Zero waits without a
/// caller deadline; accepted operations still have their separate
/// kernel-enforced expiry. The kernel rechecks results atomically with parking.
///
/// Where the cell holds `WaitCompletion` authority this parks on its `IPC`
/// source, which reports each terminal **once**: a result that was already
/// reported and is still retained does not wake the caller again, so `take`
/// what you were told about before waiting. Cells without that authority use
/// `IpcWait`, which wakes while any terminal is retained. Returns `false` on
/// timeout and on a wait error; use [`wait_any`] to tell them apart.
pub fn wait(timeout_ticks: u64) -> bool {
    matches!(
        park_for_terminal(timeout_ticks),
        Ok(Park::Notified(_) | Park::AnyTerminal)
    )
}

/// What ended one [`wait_any`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wake {
    /// A NIC receive frame is ready to drain (the net cell's source).
    NetRx,
    /// This exact owned operation reached its terminal; [`take`] it.
    Ipc(IpcOpId),
    /// The finite deadline elapsed (`TIMER` was requested).
    Timer,
    /// This MSI/MSI-X line, one the task holds from `AllocPcieIrq`, fired
    /// (`IRQ`, x86_64). Other fired lines stay pending for the next wait.
    Irq(u8),
    /// Nothing to report: a deadline elapsed without `TIMER` requested, a raw
    /// message interrupted a wait naming `NET_RX`, or a NET_RX reservation was
    /// displaced by a newer waiter.
    Interrupted,
}

/// Park on several completion sources at once and report the one that fired.
///
/// `sources` is a non-empty set of `api::completion::source::{NET_RX, TIMER,
/// IPC, IRQ}`; `TIMER` needs a nonzero `timeout_ticks`, and zero otherwise
/// waits without a deadline. Ready sources rotate, so none starves another;
/// each IPC terminal is reported once, and its outcome stays behind [`take`].
/// `IRQ` covers every line this task holds from `AllocPcieIrq`; a driver
/// passes a deadline and re-reads its device after every return, since a
/// lost or revoked interrupt is only ever noticed at the deadline.
///
/// # Errors
/// [`IpcError::Recv`] when the kernel refuses the wait (no `WaitCompletion`
/// authority, an invalid source set, `IRQ` without a held line or off x86_64,
/// or a full completion queue), and [`IpcError::Decode`] for a record whose
/// source this SDK does not know.
pub fn wait_any(sources: u32, timeout_ticks: u64) -> Result<Wake, IpcError> {
    decode_wake(sys_wait_completion_detailed(sources, timeout_ticks))
}

fn decode_wake(result: WaitCompletionResult) -> Result<Wake, IpcError> {
    match result {
        WaitCompletionResult::NoRecord => Ok(Wake::Interrupted),
        WaitCompletionResult::Completion(record) => match record.source {
            source::IPC if record.result > 0 => Ok(Wake::Ipc(record.result as usize)),
            source::NET_RX if record.result >= 0 => Ok(Wake::NetRx),
            source::NET_RX => Ok(Wake::Interrupted),
            source::TIMER => Ok(Wake::Timer),
            source::IRQ => u8::try_from(record.result)
                .map(Wake::Irq)
                .map_err(|_| IpcError::Decode),
            _ => Err(IpcError::Decode),
        },
        WaitCompletionResult::ErrorOrInvalid(_) => Err(IpcError::Recv),
    }
}

/// One park of the SDK's operation waits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Park {
    /// `WaitCompletion(IPC)` reported this operation's terminal, once.
    Notified(IpcOpId),
    /// `IpcWait` (no `WaitCompletion` authority): some owned terminal exists.
    AnyTerminal,
    /// The caller's tick budget elapsed.
    Timeout,
}

/// This Cell's compiled syscall allowlist: the exact value the loader reads
/// from `__ViCell_syscalls` at spawn, which every thread of the Cell inherits.
///
/// `declare_syscalls!`, `app_entry!` and `service_entry!` export their
/// allowlist static under this symbol; that strong definition overrides this
/// weak default. A Cell that declares no allowlist is permit-all, which the
/// default (`u64::MAX`) mirrors exactly as the loader does.
#[linkage = "weak"]
#[no_mangle]
#[allow(non_upper_case_globals)]
pub static __vicell_syscall_allowlist: u64 = u64::MAX;

/// Whether this Cell may call `WaitCompletion`, decided from its own compiled
/// allowlist so the SDK never issues a syscall it would be denied.
fn completion_wait_permitted() -> bool {
    // SAFETY: the symbol is a plain `u64` defined either here (weak default) or
    // by the Cell's allowlist macro; it is never written after load. A
    // volatile read keeps the weak value from being folded at compile time.
    let allowlist =
        unsafe { core::ptr::read_volatile(core::ptr::addr_of!(__vicell_syscall_allowlist)) };
    allowlist_permits_completion_wait(allowlist)
}

const fn allowlist_permits_completion_wait(allowlist: u64) -> bool {
    match api::syscall::ViSyscall::WaitCompletion.allowlist_bit() {
        Some(bit) => allowlist & (1u64 << bit) != 0,
        None => true,
    }
}

/// Park for an owned operation's terminal on the syscall this Cell may use:
/// the `WaitCompletion` IPC source with bit-42 authority, `IpcWait` without
/// it. Every failure of the chosen syscall is returned to the caller.
fn park_for_terminal(timeout_ticks: u64) -> Result<Park, IpcError> {
    if !completion_wait_permitted() {
        return match crate::syscall::sys_ipc_wait(timeout_ticks) {
            1 => Ok(Park::AnyTerminal),
            0 => Ok(Park::Timeout),
            _ => Err(IpcError::Recv),
        };
    }
    let sources = if timeout_ticks == 0 {
        source::IPC
    } else {
        source::IPC | source::TIMER
    };
    match decode_wake(sys_wait_completion_detailed(sources, timeout_ticks))? {
        Wake::Ipc(operation) => Ok(Park::Notified(operation)),
        Wake::Timer => Ok(Park::Timeout),
        // None of these can end an `IPC`/`IPC|TIMER` wait.
        Wake::NetRx | Wake::Irq(_) | Wake::Interrupted => Err(IpcError::Recv),
    }
}

/// Abandon one exact operation. A pre-dispatch request becomes Cancelled;
/// an already-dispatched request becomes Indeterminate (not rolled back).
/// Call `take` afterwards to release the retained terminal slot; dropping an
/// unsettled [`PendingCall`] does both.
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
/// Dropping a handle whose result was not taken stops waiting and releases its
/// kernel slot: it cancels the operation and discards the retained terminal. A
/// queued request is then never delivered; a dispatched one may still execute
/// (post-dispatch cancellation is indeterminate). To keep the slot for a later
/// [`take`] by id, read [`operation`][PendingCall::operation] and
/// `core::mem::forget` the handle.
pub struct PendingCall {
    operation: IpcOpId,
    /// Set once a `take` released the kernel slot, or found none to release.
    settled: core::cell::Cell<bool>,
}

impl Drop for PendingCall {
    fn drop(&mut self) {
        if !self.settled.get() {
            discard_call(self);
        }
    }
}

/// Whether a `take` outcome means the kernel holds no slot for the operation
/// any more: a terminal was consumed (even one whose status failed to decode)
/// or the token is unknown. `Pending` and an undersized buffer keep it.
const fn take_settles(result: &Result<IpcTakeResult, IpcError>) -> bool {
    matches!(
        result,
        Ok(IpcTakeResult::Terminal { .. }) | Err(IpcError::Decode | IpcError::InvalidOperation)
    )
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
            IpcTerminal::Reply | IpcTerminal::PreDispatchTimeout | IpcTerminal::Cancelled
        )
    }

    /// Execution effects are unresolved. PeerGone may follow dispatch, including
    /// work delegated to a sibling service task. Never retry blindly; reconcile
    /// by request identity and application policy.
    pub fn is_uncertain(&self) -> bool {
        matches!(
            self.terminal,
            IpcTerminal::Indeterminate | IpcTerminal::PeerGone
        )
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
        submit(peer_tid, request).map(|operation| Self {
            operation,
            settled: core::cell::Cell::new(false),
        })
    }

    /// The exact operation id, for correlating a terminal with the request it belongs to.
    pub const fn operation(&self) -> IpcOpId {
        self.operation
    }

    /// Take the terminal result if it has arrived; `Ok(None)` while it is still
    /// outstanding. An undersized `reply` never consumes the result.
    pub fn try_take(&self, reply: &mut [u8]) -> Result<Option<Completion>, IpcError> {
        let result = take(self.operation, reply);
        if take_settles(&result) {
            self.settled.set(true);
        }
        match result? {
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
    /// Each timed round is measured in scheduler ticks. A terminal reported for
    /// another call is left retained for that call's own `take`; with
    /// `WaitCompletion` authority it is reported once and never re-wakes this
    /// wait, and under `IpcWait` the caller yields rather than spinning on it.
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
                    if ticks == 0 {
                        break;
                    }
                    ticks
                };
                match park_for_terminal(ticks)? {
                    Park::Timeout => break,
                    // This call's own terminal, or another one: retry `take`.
                    Park::Notified(_) => {}
                    // IpcWait sees every owned terminal, including results the
                    // caller deliberately retains. Do not spend a timed round
                    // on an unrelated terminal or starve this call's provider.
                    Park::AnyTerminal => {
                        if let Some(completion) = self.try_take(reply)? {
                            return Ok(Some(completion));
                        }
                        sys_yield();
                    }
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
                    let completion = take_call(&call, reply)?.ok_or(IpcError::InvalidOperation);
                    return match completion {
                        Ok(Completion {
                            terminal: IpcTerminal::Cancelled,
                            ..
                        }) => Err(IpcError::PreDispatchTimeout),
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
        match park_for_terminal(ticks) {
            Ok(Park::Timeout | Park::Notified(_)) => {}
            Ok(Park::AnyTerminal) => other_terminal = true,
            Err(error) => {
                discard_call(&call);
                return Err(error);
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
pub fn recv_from(peer_tid: usize, recv_buf: &mut [u8]) -> Result<&[u8], IpcError> {
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
    use super::{
        completion_len, reply_sender, take_settles, Completion, IpcError, IpcTakeResult,
        IpcTerminal,
    };
    use crate::syscall::{SyscallError, SyscallResult};

    /// Phase 03e: a handle is settled (its drop issues no syscall) exactly when
    /// the kernel no longer holds a slot for it.
    #[test]
    fn only_a_consumed_or_unknown_operation_settles_its_handle() {
        let terminal = IpcTakeResult::Terminal {
            status: IpcTerminal::Indeterminate,
            len: 0,
        };
        assert!(take_settles(&Ok(terminal)));
        assert!(take_settles(&Err(IpcError::InvalidOperation)));
        // The kernel consumed the slot before the status failed to decode.
        assert!(take_settles(&Err(IpcError::Decode)));
        assert!(!take_settles(&Ok(IpcTakeResult::Pending)));
        assert!(!take_settles(&Err(IpcError::BufferTooSmall)));
    }

    #[test]
    fn masked_receive_rejects_unrelated_death_instead_of_decoding_reason() {
        assert_eq!(
            reply_sender(17, SyscallResult::Ok(31)),
            Err(IpcError::WrongSender)
        );
        assert_eq!(
            reply_sender(17, SyscallResult::Err(SyscallError::TryAgain)),
            Err(IpcError::Recv)
        );
        assert_eq!(reply_sender(17, SyscallResult::Ok(17)), Ok(()));
    }

    #[test]
    fn exact_completion_preserves_reply_length_and_terminal_outcomes() {
        assert_eq!(
            completion_len(Completion {
                terminal: IpcTerminal::Reply,
                len: 3
            }),
            Ok(3)
        );
        for (terminal, error) in [
            (IpcTerminal::PeerGone, IpcError::PeerGone),
            (
                IpcTerminal::PreDispatchTimeout,
                IpcError::PreDispatchTimeout,
            ),
            (IpcTerminal::Indeterminate, IpcError::Indeterminate),
            (IpcTerminal::Cancelled, IpcError::Cancelled),
        ] {
            assert_eq!(completion_len(Completion { terminal, len: 0 }), Err(error));
        }
        let gone = Completion {
            terminal: IpcTerminal::PeerGone,
            len: 0,
        };
        assert!(
            gone.is_uncertain(),
            "provider death does not prove no side effects"
        );
        assert!(!gone.is_definite());
        let cancelled = Completion {
            terminal: IpcTerminal::Cancelled,
            len: 0,
        };
        assert!(
            cancelled.is_definite(),
            "pre-dispatch cancellation prevents execution"
        );
        assert!(!cancelled.is_uncertain());
    }

    #[test]
    fn wait_path_follows_the_compiled_allowlist_without_probing() {
        use super::{allowlist_permits_completion_wait, completion_wait_permitted};
        use api::syscall::ViSyscall;
        let bit = ViSyscall::WaitCompletion.allowlist_bit().unwrap();
        assert_eq!(ViSyscall::WaitForEvent.allowlist_bit(), Some(bit));
        assert!(allowlist_permits_completion_wait(u64::MAX));
        assert!(allowlist_permits_completion_wait(1 << bit));
        assert!(!allowlist_permits_completion_wait(!(1u64 << bit)));
        assert!(!allowlist_permits_completion_wait(0));
        // This host test binary declares no allowlist: the weak default is the
        // loader's permit-all reading of an absent section.
        assert!(completion_wait_permitted());
    }

    #[test]
    fn completion_records_decode_to_exactly_one_wake_source() {
        use super::{decode_wake, Wake};
        use crate::syscall::WaitCompletionResult;
        use api::completion::{source, ViCompletion, RESULT_ABANDONED};
        let record = |src, result| {
            WaitCompletionResult::Completion(ViCompletion {
                slot: 3,
                source: src,
                result,
            })
        };
        assert_eq!(decode_wake(record(source::IPC, 4242)), Ok(Wake::Ipc(4242)));
        assert_eq!(decode_wake(record(source::TIMER, 0)), Ok(Wake::Timer));
        assert_eq!(
            decode_wake(record(source::NET_RX, source::NET_RX as i64)),
            Ok(Wake::NetRx)
        );
        assert_eq!(
            decode_wake(record(source::NET_RX, RESULT_ABANDONED)),
            Ok(Wake::Interrupted)
        );
        assert_eq!(decode_wake(record(source::IPC, 0)), Err(IpcError::Decode));
        assert_eq!(
            decode_wake(record(source::UNSPECIFIED, 1)),
            Err(IpcError::Decode)
        );
        assert_eq!(
            decode_wake(WaitCompletionResult::NoRecord),
            Ok(Wake::Interrupted)
        );
        assert_eq!(
            decode_wake(WaitCompletionResult::ErrorOrInvalid(-1)),
            Err(IpcError::Recv)
        );
    }

    #[test]
    fn an_irq_record_decodes_to_its_line_and_an_out_of_range_line_is_refused() {
        use super::{decode_wake, Wake};
        use crate::syscall::WaitCompletionResult;
        use api::completion::{source, ViCompletion};
        let record = |result| {
            WaitCompletionResult::Completion(ViCompletion {
                slot: 1,
                source: source::IRQ,
                result,
            })
        };
        assert_eq!(decode_wake(record(0x41)), Ok(Wake::Irq(0x41)));
        assert_eq!(decode_wake(record(0)), Ok(Wake::Irq(0)));
        assert_eq!(decode_wake(record(255)), Ok(Wake::Irq(255)));
        assert_eq!(decode_wake(record(256)), Err(IpcError::Decode));
        assert_eq!(decode_wake(record(-1)), Err(IpcError::Decode));
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
