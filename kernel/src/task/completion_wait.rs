//! The wait side of the completion queue: `WaitCompletion`.
//!
//! Calling this **is** the submission. A hardware condition is not a discrete
//! operation a cell submits, so there is nothing else that could reserve a slot
//! from the waiting cell's own context — and reserving anywhere else would put
//! the refusal in an interrupt handler, which has nothing to do with it. The
//! wait therefore reserves, records the reservation against the source, and only
//! then parks.
//!
//! The order of those steps is the whole correctness argument. Arming before
//! checking whether a frame is already pending means the two possible arrivals
//! are both covered: one that lands after the arm completes the reservation, and
//! one that landed before it left the level flag set, which the check consumes.
//! There is no third case, and no window in which a frame is visible to neither.
//!
//! A wait that ends without a result releases its slot before returning. A
//! reservation that outlived its waiter would consume queue capacity that never
//! comes back, and the queue is the cell's bound on outstanding work.

use super::completion::{self, Completion, CompletionQueue};
use super::syscall::{validate_user_buf, SyscallError};
use super::tcb::{CompletionWait, Task, TaskState};
use super::waker;
use crate::dev::irq_wait::{self, IrqLineSet};
use api::completion::{ViCompletion, COMPLETION_LEN};

/// Removes this waiter's registration on every return path without erasing a
/// newer waiter that may have replaced it on the shared per-cell queue.
pub(super) struct WaiterRegistration<'a> {
    queue: &'a CompletionQueue,
    tid: usize,
    slot: super::completion::SlotId,
}

impl<'a> WaiterRegistration<'a> {
    #[cfg(not(cellos_posture = "release"))]
    /// A registration for a wait that does not name `IRQ`.
    pub(super) fn new(
        queue: &'a CompletionQueue,
        tid: usize,
        slot: super::completion::SlotId,
        source: u32,
    ) -> Option<Self> {
        Self::with_irq_lines(queue, tid, slot, source, IrqLineSet::EMPTY)
    }

    /// `irq_lines` is the wait's IRQ snapshot, published in the same
    /// `SCHEDULER` hold as the slot so the sweep never sees one without the
    /// other.
    pub(super) fn with_irq_lines(
        queue: &'a CompletionQueue,
        tid: usize,
        slot: super::completion::SlotId,
        source: u32,
        irq_lines: IrqLineSet,
    ) -> Option<Self> {
        queue.register_waiter(tid);
        let registered = {
            let mut guard = super::SCHEDULER.lock();
            guard
                .as_mut()
                .and_then(|sched| sched.tasks.get_mut(&tid))
                .map(|task| {
                    task.completion_wait = Some(CompletionWait {
                        source,
                        slot,
                        irq_lines,
                    });
                })
                .is_some()
        };
        if registered {
            Some(Self { queue, tid, slot })
        } else {
            let _ = queue.clear_waiter(tid);
            None
        }
    }
}

impl Drop for WaiterRegistration<'_> {
    fn drop(&mut self) {
        let _ = self.queue.clear_waiter(self.tid);
        let mut guard = super::SCHEDULER.lock();
        if let Some(task) = guard
            .as_mut()
            .and_then(|sched| sched.tasks.get_mut(&self.tid))
        {
            if task.completion_wait.map(|wait| wait.slot) == Some(self.slot) {
                task.completion_wait = None;
            }
        }
    }
}

/// Validate the source/deadline pair before a queue slot is reserved.
///
/// The mask is a non-empty subset of the public sources; `TIMER` needs a
/// finite deadline to complete against. `IRQ` lines exist only on x86_64
/// (MSI/MSI-X from `AllocPcieIrq`), so every other kernel refuses the bit.
pub(super) fn source_is_valid(mask: u32, deadline: Option<u64>) -> bool {
    api::completion::source::is_wait_mask_supported(mask)
        && (mask & api::completion::source::TIMER == 0 || deadline.is_some())
        && (cfg!(target_arch = "x86_64") || mask & api::completion::source::IRQ == 0)
}

/// Rotation order across completion sources (CP1 §5.5; IRQ per 04b CP1 §3.3).
pub(super) const SELECTION_ORDER: [u32; 4] = [
    api::completion::source::NET_RX,
    api::completion::source::IPC,
    api::completion::source::IRQ,
    api::completion::source::TIMER,
];

/// A fresh task's rotor names TIMER as last delivered, so the first
/// multi-source selection starts at NET_RX.
pub(super) const INITIAL_ROTOR: u8 = (SELECTION_ORDER.len() - 1) as u8;

/// The ready source that follows `rotor` in [`SELECTION_ORDER`], with its index.
///
/// A source stays eligible until it is delivered, and each delivery moves the
/// rotor past it, so every ready source is chosen within four selections.
pub(super) fn select_source(rotor: u8, ready: u32) -> Option<(u32, u8)> {
    (1..=SELECTION_ORDER.len()).find_map(|step| {
        let index = (rotor as usize + step) % SELECTION_ORDER.len();
        let source = SELECTION_ORDER[index];
        (ready & source != 0).then_some((source, index as u8))
    })
}

/// The pending lines of this task's IRQ snapshot (empty when the wait does not
/// name `IRQ`). Atomics only, so it is safe under `SCHEDULER`: it is the
/// selector's readiness, the pre-park re-check, and the sweep's wake test.
fn irq_lines_ready(task: &Task) -> IrqLineSet {
    task.completion_wait
        .as_ref()
        .map_or(IrqLineSet::EMPTY, |wait| {
            irq_wait::pending_in(&wait.irq_lines)
        })
}

/// Whether the scheduler sweep must ready a task parked in `WaitCompletion`
/// with `source`: the mask names `IRQ` and a snapshot line is pending. A
/// pending line outside the snapshot never wakes it.
pub(crate) fn irq_wake_due(task: &Task, source: u32) -> bool {
    source & api::completion::source::IRQ != 0 && !irq_lines_ready(task).is_empty()
}

/// One record chosen for a multi-source completion wait.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct Selection {
    /// The single source bit this record reports.
    pub source: u32,
    /// `NET_RX` mask for NET_RX, the operation token for IPC, the line for
    /// IRQ, 0 for TIMER.
    pub result: isize,
    /// A ready NET_RX observation lost the selection and must be returned to
    /// the level flag ([`super::waker::relevel_net_rx`]) before the wait returns.
    pub relevel_net_rx: bool,
    /// Operation records examined for the IPC candidate (0 when IPC not chosen).
    pub scanned: usize,
}

/// Choose the one record a multi-source wait returns (CP1 §5.3-5.5).
///
/// The caller resolves NET_RX ownership and deadline expiry *outside*
/// `SCHEDULER` (NET_RX's leaf lock is never taken under it) and passes the
/// results in; this function runs under `SCHEDULER`, where every IPC terminal
/// transition is also serialized. A chosen IPC record consumes exactly one
/// operation's one-shot notification; a ready IPC source that loses stays
/// latched; TIMER is this call's deadline and is not carried over. IRQ
/// readiness is peeked here from the task's snapshot (atomics only); a chosen
/// line is the first ready one after `completion_irq_cursor`, and it is *not*
/// consumed here: the caller takes it with the ownership check outside
/// `SCHEDULER`, and a line that loses stays pending.
pub(super) fn select_locked(
    task: &mut Task,
    mask: u32,
    net_rx_ready: bool,
    timer_expired: bool,
) -> Option<Selection> {
    let mut ready = 0;
    if net_rx_ready && mask & api::completion::source::NET_RX != 0 {
        ready |= api::completion::source::NET_RX;
    }
    if mask & api::completion::source::IPC != 0 && super::async_ipc::has_pending_notification(task)
    {
        ready |= api::completion::source::IPC;
    }
    let irq_ready = if mask & api::completion::source::IRQ != 0 {
        irq_lines_ready(task)
    } else {
        IrqLineSet::EMPTY
    };
    if !irq_ready.is_empty() {
        ready |= api::completion::source::IRQ;
    }
    if timer_expired && mask & api::completion::source::TIMER != 0 {
        ready |= api::completion::source::TIMER;
    }
    let (source, index) = select_source(task.completion_rotor, ready)?;
    let (result, scanned) = if source == api::completion::source::IPC {
        let notification = super::async_ipc::take_notification(task)?;
        (notification.token as isize, notification.scanned)
    } else if source == api::completion::source::NET_RX {
        (api::completion::source::NET_RX as isize, 0)
    } else if source == api::completion::source::IRQ {
        let line = irq_ready.next_after(task.completion_irq_cursor)?;
        task.completion_irq_cursor = line;
        (line as isize, 0)
    } else {
        (0, 0)
    };
    task.completion_rotor = index;
    Some(Selection {
        source,
        result,
        relevel_net_rx: ready & api::completion::source::NET_RX != 0
            && source != api::completion::source::NET_RX,
        scanned,
    })
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum CompletionParkDecision {
    Parked,
    MailboxPending,
    /// An owned IPC terminal still owes its one-shot notification.
    IpcPending,
    /// A line of the wait's IRQ snapshot is already pending.
    IrqPending,
}

/// Publish a completion wait only if no already-published IPC must interrupt it.
///
/// The caller holds `SCHEDULER`; keeping the mailbox predicate, outgoing
/// context handoff, and state write in this one critical section closes both
/// enqueue-before-park orderings and the state-before-yield SMP window. TIMER
/// is deliberately deadline-only and therefore ignores the mailbox. The IPC
/// source checks its pending notification in the same hold: every terminal
/// transition takes `SCHEDULER` too, so a terminal published before this check
/// is seen here and one published after it finds the parked state and wakes it.
pub(super) fn publish_wait_state_locked(
    task: &mut Task,
    tid: usize,
    source: u32,
    deadline: Option<u64>,
) -> CompletionParkDecision {
    if source & api::completion::source::NET_RX != 0 && !task.pending_msgs.is_empty() {
        return CompletionParkDecision::MailboxPending;
    }
    if source & api::completion::source::IPC != 0
        && super::async_ipc::has_pending_notification(task)
    {
        return CompletionParkDecision::IpcPending;
    }
    // An MSI that fires after this check finds the published state, and the
    // sweep (triggered early by the MSI wake hint) readies the task.
    if irq_wake_due(task, source) {
        return CompletionParkDecision::IrqPending;
    }
    super::arm_ipc_block_handoff(tid);
    task.state = TaskState::WaitCompletion { source, deadline };
    CompletionParkDecision::Parked
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum NetRxCleanupStep {
    NoRecord,
    Record(Completion),
    RetryCompleting,
}

/// Resolve one ownership transition at the end of a NET_RX wait.
///
/// `Completing` is not an empty result: the source owns the reservation and
/// must finish publishing before the waiter may decide between a record and
pub(super) fn net_rx_cleanup_step(queue: &alloc::sync::Arc<CompletionQueue>) -> NetRxCleanupStep {
    match waker::disarm_net_rx(queue) {
        waker::DisarmResult::Owned(slot) if queue.release(slot) => NetRxCleanupStep::NoRecord,
        waker::DisarmResult::Completing => NetRxCleanupStep::RetryCompleting,
        _ => queue
            .drain()
            .map(NetRxCleanupStep::Record)
            .unwrap_or(NetRxCleanupStep::NoRecord),
    }
}

pub(super) fn finish_net_rx_wait(
    caller_id: usize,
    out_ptr: usize,
    queue: &alloc::sync::Arc<CompletionQueue>,
) -> Result<usize, SyscallError> {
    loop {
        match net_rx_cleanup_step(queue) {
            NetRxCleanupStep::NoRecord => return Ok(0),
            NetRxCleanupStep::Record(done) => {
                return write_completion(caller_id, out_ptr, done);
            }
            NetRxCleanupStep::RetryCompleting => core::hint::spin_loop(),
        }
    }
}

/// Reserve a slot on the caller's completion queue, wait for it to be filled,
/// and write the result to `out_ptr`.
///
/// `mask` is a non-empty subset of `NET_RX | TIMER | IPC | IRQ`. `deadline` is
/// an absolute tick count; `None` is invalid when the mask names `TIMER`, which
/// needs a finite instant to complete against. A lone `NET_RX` or `TIMER`
/// keeps its original single-source path; every other mask goes through
/// [`wait_multi`], which returns exactly one source per call. `IRQ` covers
/// every line the caller holds from `AllocPcieIrq` when the call starts.
///
/// # Returns
/// `1` when a completion was written to `out_ptr`, `0` when the wait ended with
/// nothing to report.
///
/// # Errors
/// - `InvalidInput` — `mask` is empty or names a source this kernel does not
///   serve (`IRQ` off x86_64), `TIMER` lacks a deadline, or `out_ptr` is null.
/// - `PermissionDenied` — `IRQ` is named and the caller holds no line, at
///   entry or after every line it held was revoked mid-wait.
/// - `BufferTooSmall` — `out_ptr` cannot hold a whole record.
/// - `TryAgain` — the caller's queue has no free slot. This is backpressure, and
///   it is reported here, to the cell that caused it, rather than to the
///   interrupt handler that would otherwise have had nowhere to put a result.
/// - `Unknown` — the caller is not a live task.
///
/// # Panics
/// Never panics.
pub fn wait_completion(
    caller_id: usize,
    mask: u32,
    deadline: Option<u64>,
    out_ptr: usize,
) -> Result<usize, SyscallError> {
    // Every source in the mask shares the one reservation made below; a wait
    // returns at most one record, so one landing place is always enough.
    if !source_is_valid(mask, deadline) {
        return Err(SyscallError::InvalidInput);
    }
    validate_user_buf(out_ptr, COMPLETION_LEN, COMPLETION_LEN)?;
    // The snapshot takes the allocation lock alone, before any SCHEDULER hold.
    let irq_lines = if mask & api::completion::source::IRQ != 0 {
        let lines = owned_irq_lines(caller_id);
        if lines.is_empty() {
            return Err(SyscallError::PermissionDenied);
        }
        lines
    } else {
        IrqLineSet::EMPTY
    };

    let queue = {
        let mut guard = super::SCHEDULER.lock();
        let sched = guard.as_mut().ok_or(SyscallError::Unknown)?;
        completion::queue_for(sched, caller_id).ok_or(SyscallError::Unknown)?
    };
    let slot = queue.reserve().ok_or(SyscallError::TryAgain)?;
    // Exit cleanup keys on who owns the slot: a NET_RX reservation is owned by
    // the waker (displaced later as abandoned); otherwise only this waiter holds
    // it and task exit must release it, exactly as for a lone TIMER wait.
    let slot_owner = if mask & api::completion::source::NET_RX != 0 {
        api::completion::source::NET_RX
    } else {
        api::completion::source::TIMER
    };
    let registration =
        WaiterRegistration::with_irq_lines(&queue, caller_id, slot, slot_owner, irq_lines);
    let _waiter = match registration {
        Some(waiter) => waiter,
        None => {
            let _ = queue.release(slot);
            return Err(SyscallError::Unknown);
        }
    };
    if mask != api::completion::source::NET_RX && mask != api::completion::source::TIMER {
        return wait_multi(caller_id, mask, deadline, out_ptr, &queue, slot);
    }
    let is_net_rx = mask == api::completion::source::NET_RX;
    if is_net_rx {
        waker::arm_net_rx(queue.clone(), slot);
    }

    // Lost-wakeup guard, first half: a frame that arrived while nobody held a
    // reservation is remembered in the level flag. Consuming it only *after* the
    // arm is what makes the check total — a frame arriving during the check has
    // a reservation to complete instead.
    if is_net_rx
        && waker::consume_pending(mask) != 0
        && matches!(waker::disarm_net_rx(&queue), waker::DisarmResult::Owned(_))
    {
        // The result is reported straight out of the reservation rather than
        // pushed through the queue and pulled back: appending would raise a
        // wake request for a task that is not parked, and that request would
        // still be outstanding to cancel the *next* park.
        if queue.release(slot) {
            return write_completion(
                caller_id,
                out_ptr,
                Completion {
                    slot,
                    source: api::completion::source::NET_RX,
                    result: mask as isize,
                },
            );
        }
    }

    // Second half: anything the source landed while the steps above ran is
    // delivered without parking at all.
    if let Some(done) = queue.drain() {
        if !is_net_rx {
            let _ = queue.release(slot);
        }
        return write_completion(caller_id, out_ptr, done);
    }

    loop {
        // The mailbox predicate and the completion-wait publication share the
        // producer's scheduler critical section. An IPC already queued for a
        // NET_RX waiter therefore selects the existing raw no-record return
        // rather than allowing the task to park behind that message.
        let park = {
            let mut guard = super::SCHEDULER.lock();
            guard
                .as_mut()
                .and_then(|sched| sched.tasks.get_mut(&caller_id))
                .map(|task| publish_wait_state_locked(task, caller_id, mask, deadline))
        };
        match park {
            Some(CompletionParkDecision::MailboxPending) => {
                return finish_net_rx_wait(caller_id, out_ptr, &queue);
            }
            // A lone NET_RX or TIMER mask never names IPC or IRQ, so these
            // gates cannot fire here; they resolve ownership like a
            // record-less interruption.
            Some(CompletionParkDecision::IpcPending | CompletionParkDecision::IrqPending) => {
                if is_net_rx {
                    return finish_net_rx_wait(caller_id, out_ptr, &queue);
                }
                let _ = queue.release(slot);
                return Ok(0);
            }
            Some(CompletionParkDecision::Parked) => {}
            None => {
                if is_net_rx {
                    while let NetRxCleanupStep::RetryCompleting = net_rx_cleanup_step(&queue) {
                        core::hint::spin_loop();
                    }
                } else {
                    let _ = queue.release(slot);
                }
                return Err(SyscallError::Unknown);
            }
        }
        super::yield_cpu();

        if let Some(done) = queue.drain() {
            if !is_net_rx {
                let _ = queue.release(slot);
            }
            return write_completion(caller_id, out_ptr, done);
        }

        if !is_net_rx {
            // TIMER has no interrupt-side producer. The existing deadline sweep
            // parks and wakes the task; after resumption the submitter releases
            // its own slot and writes the synthetic completion directly. Going
            // through `complete` here would raise a wake for the already-running
            // task and cancel its next unrelated park.
            let expired = deadline
                .map(|at| super::system_ticks() as u64 >= at)
                .unwrap_or(false);
            if expired && queue.release(slot) {
                return write_completion(
                    caller_id,
                    out_ptr,
                    Completion {
                        slot,
                        source: api::completion::source::TIMER,
                        result: 0,
                    },
                );
            }
            continue;
        }

        // A deadline/spurious wake and an IPC interruption all use the same
        // ownership resolution. A concurrent source publication wins over the
        // raw-zero path and is drained as a genuine NET_RX record.
        return finish_net_rx_wait(caller_id, out_ptr, &queue);
    }
}

/// The multi-source wait: any mask other than a lone `NET_RX` or `TIMER`.
///
/// One reservation (`slot`) serves every source. Each round resolves NET_RX
/// readiness outside `SCHEDULER` (its leaf lock is never taken under it),
/// then selects under `SCHEDULER` with [`select_locked`], where every IPC
/// terminal transition is serialized too. A chosen IRQ line is taken outside
/// `SCHEDULER` under the allocation lock, which re-checks ownership. Nothing
/// ready parks through [`publish_wait_state_locked`], which re-checks the
/// mailbox, pending IPC notifications and the IRQ snapshot in the same hold
/// that publishes the state. Exactly one source is reported; a NET_RX
/// observation that is not reported goes back to the level flag, an IPC
/// notification that is not reported stays latched, and so does an IRQ line.
fn wait_multi(
    caller_id: usize,
    mask: u32,
    deadline: Option<u64>,
    out_ptr: usize,
    queue: &alloc::sync::Arc<CompletionQueue>,
    slot: super::completion::SlotId,
) -> Result<usize, SyscallError> {
    let has_net_rx = mask & api::completion::source::NET_RX != 0;
    if has_net_rx {
        waker::arm_net_rx(queue.clone(), slot);
    }
    loop {
        // Consuming the level flag only after the arm keeps the lost-wakeup
        // argument of the single-source path: a frame either completed the
        // reservation (drainable) or left the flag set.
        let net_rx_flag =
            has_net_rx && waker::consume_pending(api::completion::source::NET_RX) != 0;
        let net_rx_ready = net_rx_flag || (has_net_rx && queue.drainable() != 0);
        let timer_expired = deadline.is_some_and(|at| super::system_ticks() as u64 >= at);
        let chosen = {
            let mut guard = super::SCHEDULER.lock();
            guard
                .as_mut()
                .and_then(|sched| sched.tasks.get_mut(&caller_id))
                .map(|task| select_locked(task, mask, net_rx_ready, timer_expired))
        };
        match chosen {
            None => return abandon_multi(queue, slot, has_net_rx, net_rx_ready),
            Some(Some(selection))
                if selection.source == api::completion::source::IRQ
                    && !take_owned_line(caller_id, selection.result as u8) =>
            {
                // Revoked between peek and take. The flag observation goes
                // back; a completed reservation is still drainable next round.
                if net_rx_flag {
                    waker::relevel_net_rx();
                }
                let lines = irq_lines_after_revoke(caller_id, queue, slot, has_net_rx)?;
                let refreshed = {
                    let mut guard = super::SCHEDULER.lock();
                    guard
                        .as_mut()
                        .and_then(|sched| sched.tasks.get_mut(&caller_id))
                        .map(|task| {
                            if let Some(wait) = task.completion_wait.as_mut() {
                                wait.irq_lines = lines;
                            }
                        })
                };
                if refreshed.is_none() {
                    return abandon_multi(queue, slot, has_net_rx, false);
                }
                continue;
            }
            Some(Some(selection)) => {
                return finish_multi(caller_id, out_ptr, queue, slot, has_net_rx, selection);
            }
            Some(None) => {}
        }
        // Nothing is ready. A deadline without TIMER ends the wait with no
        // record, exactly like a lone NET_RX deadline.
        if timer_expired {
            return finish_multi_empty(caller_id, out_ptr, queue, slot, has_net_rx);
        }
        let park = {
            let mut guard = super::SCHEDULER.lock();
            guard
                .as_mut()
                .and_then(|sched| sched.tasks.get_mut(&caller_id))
                .map(|task| publish_wait_state_locked(task, caller_id, mask, deadline))
        };
        match park {
            None => return abandon_multi(queue, slot, has_net_rx, false),
            Some(CompletionParkDecision::MailboxPending) => {
                return finish_multi_empty(caller_id, out_ptr, queue, slot, has_net_rx);
            }
            Some(CompletionParkDecision::IpcPending | CompletionParkDecision::IrqPending) => {
                continue
            }
            Some(CompletionParkDecision::Parked) => super::yield_cpu(),
        }
    }
}

/// The lines the caller still holds after a take found its chosen line
/// revoked. None left ends the wait: the reservation is released (any NET_RX
/// evidence goes back to the level flag) and the call reports
/// `PermissionDenied`, exactly as if it had started without lines.
fn irq_lines_after_revoke(
    caller_id: usize,
    queue: &alloc::sync::Arc<CompletionQueue>,
    slot: super::completion::SlotId,
    has_net_rx: bool,
) -> Result<IrqLineSet, SyscallError> {
    let lines = owned_irq_lines(caller_id);
    if !lines.is_empty() {
        return Ok(lines);
    }
    if has_net_rx {
        if settle_unreported_net_rx(queue) {
            waker::relevel_net_rx();
        }
    } else {
        let _ = queue.release(slot);
    }
    Err(SyscallError::PermissionDenied)
}

/// Every `AllocPcieIrq` line `tid` holds. Takes the allocation lock alone.
#[cfg(target_arch = "x86_64")]
fn owned_irq_lines(tid: usize) -> IrqLineSet {
    crate::dev::pcie_msi::lines_owned_by(tid)
}

#[cfg(target_arch = "x86_64")]
fn take_owned_line(tid: usize, line: u8) -> bool {
    crate::dev::pcie_msi::take_owned(tid, line)
}

#[cfg(target_arch = "x86_64")]
fn repend_owned_line(tid: usize, line: u8) {
    crate::dev::pcie_msi::repend_owned(tid, line)
}

// Off x86_64 `source_is_valid` refuses `IRQ`, so no wait ever holds a line.
#[cfg(not(target_arch = "x86_64"))]
fn owned_irq_lines(_tid: usize) -> IrqLineSet {
    IrqLineSet::EMPTY
}

#[cfg(not(target_arch = "x86_64"))]
fn take_owned_line(_tid: usize, _line: u8) -> bool {
    false
}

#[cfg(not(target_arch = "x86_64"))]
fn repend_owned_line(_tid: usize, _line: u8) {}

/// Report `selection` and settle every other source's ownership.
///
/// A record that cannot be copied out is not lost: an IPC notification is
/// re-armed for the next wait, a NET_RX observation goes back to the level
/// flag, and an IRQ line is re-pended if the caller still holds it. TIMER is
/// this call's deadline and has nothing to restore.
fn finish_multi(
    caller_id: usize,
    out_ptr: usize,
    queue: &alloc::sync::Arc<CompletionQueue>,
    slot: super::completion::SlotId,
    has_net_rx: bool,
    selection: Selection,
) -> Result<usize, SyscallError> {
    let reported = settle_multi(queue, slot, has_net_rx, selection);
    let written = write_completion(caller_id, out_ptr, reported);
    if written.is_err() {
        if reported.source == api::completion::source::IPC {
            let mut guard = super::SCHEDULER.lock();
            if let Some(task) = guard
                .as_mut()
                .and_then(|sched| sched.tasks.get_mut(&caller_id))
            {
                let _ = super::async_ipc::restore_notification(task, reported.result as usize);
            }
        } else if reported.source == api::completion::source::NET_RX {
            waker::relevel_net_rx();
        } else if reported.source == api::completion::source::IRQ {
            repend_owned_line(caller_id, reported.result as u8);
        }
    }
    written
}

/// Settle every source's ownership for `selection` and build its record.
///
/// Runs outside `SCHEDULER`: it takes only the NET_RX leaf lock and the
/// queue's own lock. On return the reservation is released, any NET_RX
/// evidence not reported is back in the level flag, and no reservation is
/// left armed for this waiter.
pub(super) fn settle_multi(
    queue: &alloc::sync::Arc<CompletionQueue>,
    slot: super::completion::SlotId,
    has_net_rx: bool,
    selection: Selection,
) -> Completion {
    if selection.source == api::completion::source::NET_RX {
        // The reservation either completed (its record is reported) or the
        // level flag was the evidence (the released slot carries the record).
        return loop {
            match net_rx_cleanup_step(queue) {
                NetRxCleanupStep::Record(done) => break done,
                NetRxCleanupStep::NoRecord => {
                    break Completion {
                        slot,
                        source: api::completion::source::NET_RX,
                        result: selection.result,
                    }
                }
                NetRxCleanupStep::RetryCompleting => core::hint::spin_loop(),
            }
        };
    }
    let mut relevel = selection.relevel_net_rx;
    if has_net_rx {
        relevel |= settle_unreported_net_rx(queue);
    } else {
        // Only this waiter holds the slot; no source can complete it.
        let _ = queue.release(slot);
    }
    if relevel {
        waker::relevel_net_rx();
    }
    Completion {
        slot,
        source: selection.source,
        result: selection.result,
    }
}

/// Disarm this waiter's NET_RX reservation; `true` when a frame completed it
/// meanwhile (that record is drained here and must go back to the level flag).
fn settle_unreported_net_rx(queue: &alloc::sync::Arc<CompletionQueue>) -> bool {
    loop {
        match net_rx_cleanup_step(queue) {
            NetRxCleanupStep::NoRecord => return false,
            NetRxCleanupStep::Record(_) => return true,
            NetRxCleanupStep::RetryCompleting => core::hint::spin_loop(),
        }
    }
}

/// End a multi-source wait that has nothing to report (deadline without TIMER,
/// or a raw message interrupting a mask that names NET_RX). A NET_RX record
/// that landed meanwhile is still reported, as on the single-source path.
fn finish_multi_empty(
    caller_id: usize,
    out_ptr: usize,
    queue: &alloc::sync::Arc<CompletionQueue>,
    slot: super::completion::SlotId,
    has_net_rx: bool,
) -> Result<usize, SyscallError> {
    if has_net_rx {
        return finish_net_rx_wait(caller_id, out_ptr, queue);
    }
    let _ = queue.release(slot);
    Ok(0)
}

/// The caller vanished mid-wait: release the reservation and return any NET_RX
/// evidence to the level flag for the next waiter.
fn abandon_multi(
    queue: &alloc::sync::Arc<CompletionQueue>,
    slot: super::completion::SlotId,
    has_net_rx: bool,
    net_rx_seen: bool,
) -> Result<usize, SyscallError> {
    let mut relevel = net_rx_seen;
    if has_net_rx {
        relevel |= settle_unreported_net_rx(queue);
    } else {
        let _ = queue.release(slot);
    }
    if relevel {
        waker::relevel_net_rx();
    }
    Err(SyscallError::Unknown)
}

#[cfg(test)]
mod tests {
    //! NET_RX ownership settlement of the multi-source wait, against the real
    //! global reservation and level flag (serialized by `TEST_STATE_LOCK`).

    use super::*;
    use crate::sched::scheduler::Scheduler;
    use alloc::boxed::Box;
    use alloc::sync::Arc;
    use types::CellId;

    const NET_RX: u32 = api::completion::source::NET_RX;
    const IPC: u32 = api::completion::source::IPC;
    const TID: usize = 72_201;

    fn queue() -> Arc<CompletionQueue> {
        let mut sched = Scheduler::new();
        sched.tasks.insert(
            TID,
            Box::new(Task::new(
                TID,
                CellId(72),
                "msw-settle",
                alloc::vec::Vec::new(),
            )),
        );
        let queue = completion::queue_for(&mut sched, TID).expect("queue");
        // Leave no level-flag residue from another test.
        let _ = waker::consume_pending(NET_RX);
        queue
    }

    fn ipc_selection(token: isize) -> Selection {
        Selection {
            source: IPC,
            result: token,
            relevel_net_rx: false,
            scanned: 1,
        }
    }

    fn settled(queue: &CompletionQueue) -> bool {
        queue.reserved() == 0 && queue.drainable() == 0
    }

    #[test]
    fn ipc_wins_over_a_completed_net_rx_reservation_and_relevels_it() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        let queue = queue();
        let slot = queue.reserve().unwrap();
        waker::arm_net_rx(queue.clone(), slot);
        let pending = waker::begin_signal_net_rx_for_test().expect("armed reservation");
        waker::finish_signal_net_rx_for_test(pending);
        assert_eq!(queue.drainable(), 1, "the frame completed the reservation");

        let record = settle_multi(&queue, slot, true, ipc_selection(77));
        assert_eq!(
            record,
            Completion {
                slot,
                source: IPC,
                result: 77
            }
        );
        assert!(settled(&queue), "reservation drained and released");
        assert_ne!(
            waker::consume_pending(NET_RX),
            0,
            "the unreported frame is back in the level flag"
        );
    }

    #[test]
    fn ipc_win_waits_out_an_in_flight_net_rx_completion() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        let queue = queue();
        let slot = queue.reserve().unwrap();
        waker::arm_net_rx(queue.clone(), slot);
        let pending = waker::begin_signal_net_rx_for_test().expect("armed reservation");
        assert_eq!(
            net_rx_cleanup_step(&queue),
            NetRxCleanupStep::RetryCompleting,
            "the signal owns the reservation mid-publication"
        );
        let signaller = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(20));
            waker::finish_signal_net_rx_for_test(pending);
        });
        let record = settle_multi(&queue, slot, true, ipc_selection(78));
        signaller.join().unwrap();
        assert_eq!(record.source, IPC);
        assert_eq!(record.result, 78);
        assert!(settled(&queue));
        assert_ne!(waker::consume_pending(NET_RX), 0);
    }

    #[test]
    fn ipc_win_with_an_untouched_reservation_releases_without_relevel() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        let queue = queue();
        let slot = queue.reserve().unwrap();
        waker::arm_net_rx(queue.clone(), slot);
        let record = settle_multi(&queue, slot, true, ipc_selection(79));
        assert_eq!(record.result, 79);
        assert!(settled(&queue));
        assert_eq!(waker::consume_pending(NET_RX), 0, "no frame, no flag");
    }

    #[test]
    fn net_rx_selected_from_the_level_flag_reports_through_the_released_slot() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        let queue = queue();
        let slot = queue.reserve().unwrap();
        waker::arm_net_rx(queue.clone(), slot);
        let selection = Selection {
            source: NET_RX,
            result: NET_RX as isize,
            relevel_net_rx: false,
            scanned: 0,
        };
        let record = settle_multi(&queue, slot, true, selection);
        assert_eq!(
            record,
            Completion {
                slot,
                source: NET_RX,
                result: NET_RX as isize
            }
        );
        assert!(settled(&queue));
    }

    #[test]
    fn abandon_settles_a_completed_reservation_and_a_private_slot() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        let queue = queue();
        let slot = queue.reserve().unwrap();
        waker::arm_net_rx(queue.clone(), slot);
        let pending = waker::begin_signal_net_rx_for_test().expect("armed reservation");
        waker::finish_signal_net_rx_for_test(pending);
        assert_eq!(
            abandon_multi(&queue, slot, true, false),
            Err(SyscallError::Unknown)
        );
        assert!(settled(&queue));
        assert_ne!(waker::consume_pending(NET_RX), 0);

        let private = queue.reserve().unwrap();
        assert_eq!(
            abandon_multi(&queue, private, false, false),
            Err(SyscallError::Unknown)
        );
        assert!(settled(&queue));
        assert_eq!(waker::consume_pending(NET_RX), 0);
    }

    #[test]
    fn the_waiter_table_is_preallocated_so_registration_never_grows_it() {
        let queue = queue();
        for tid in 0..completion::MAX_WAITERS {
            queue.register_waiter(1_000 + tid);
        }
        assert!(queue.waiter_capacity() >= completion::MAX_WAITERS);
        assert_eq!(queue.waiter_capacity(), completion::MAX_WAITERS);
    }

    // ---- IRQ source (04b CP1 §3.3-§3.4, §9) ----

    const IRQ: u32 = api::completion::source::IRQ;
    const TIMER: u32 = api::completion::source::TIMER;

    /// A task mid-`WaitCompletion` whose snapshot holds `lines`.
    fn irq_waiter(lines: &[u8], slot: completion::SlotId) -> Box<Task> {
        let mut task = Box::new(Task::new(
            TID + 1,
            CellId(72),
            "msw-irq",
            alloc::vec::Vec::new(),
        ));
        let mut irq_lines = IrqLineSet::EMPTY;
        for line in lines {
            irq_lines.insert(*line);
        }
        task.completion_wait = Some(CompletionWait {
            source: TIMER,
            slot,
            irq_lines,
        });
        task
    }

    fn select_all(task: &mut Task, mask: u32, rounds: usize) -> alloc::vec::Vec<(u32, isize)> {
        (0..rounds)
            .map(|_| {
                let chosen = select_locked(task, mask, true, true).expect("a ready source");
                (chosen.source, chosen.result)
            })
            .collect()
    }

    #[test]
    fn four_source_rotor_serves_irq_between_net_rx_and_timer_without_consuming_it() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        let queue = queue();
        let slot = queue.reserve().unwrap();
        let mut task = irq_waiter(&[0xC0, 0xC1], slot);
        assert_eq!(task.completion_rotor, INITIAL_ROTOR);
        irq_wait::restore_pending(0xC0);
        irq_wait::restore_pending(0xC1);
        let picks = select_all(&mut task, NET_RX | IRQ | TIMER, 6);
        assert_eq!(
            picks,
            [
                (NET_RX, NET_RX as isize),
                (IRQ, 0xC0),
                (TIMER, 0),
                (NET_RX, NET_RX as isize),
                (IRQ, 0xC1),
                (TIMER, 0),
            ]
        );
        assert!(
            irq_wait::take_pending(0xC0) && irq_wait::take_pending(0xC1),
            "selection only peeks; the waiter takes the line"
        );
        assert_eq!(select_locked(&mut task, IRQ, false, false), None);
        let _ = queue.release(slot);
    }

    #[test]
    fn irq_lines_are_served_round_robin_after_the_cursor() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        let queue = queue();
        let slot = queue.reserve().unwrap();
        let mut task = irq_waiter(&[0xC2, 0xC3, 0xC6], slot);
        for line in [0xC2, 0xC3, 0xC6] {
            irq_wait::restore_pending(line);
        }
        let lines: alloc::vec::Vec<isize> = select_all(&mut task, IRQ, 4)
            .into_iter()
            .map(|(source, line)| {
                assert_eq!(source, IRQ);
                line
            })
            .collect();
        assert_eq!(
            lines,
            [0xC2, 0xC3, 0xC6, 0xC2],
            "a busy line cannot starve the others"
        );
        assert!(irq_wait::take_pending(0xC3));
        assert_eq!(select_all(&mut task, IRQ, 1), [(IRQ, 0xC6)]);
        assert!(irq_wait::take_pending(0xC2) && irq_wait::take_pending(0xC6));
        let _ = queue.release(slot);
    }

    #[test]
    fn a_pending_snapshot_line_stops_a_park_and_wakes_the_sweep_but_an_outside_line_does_not() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        let queue = queue();
        let slot = queue.reserve().unwrap();
        let mut task = irq_waiter(&[0xC4], slot);
        task.state = TaskState::Ready;
        assert!(!irq_wake_due(&task, IRQ | TIMER));
        irq_wait::restore_pending(0xC5);
        assert!(
            !irq_wake_due(&task, IRQ | TIMER),
            "a pending line outside the snapshot never wakes the waiter"
        );
        irq_wait::restore_pending(0xC4);
        assert!(irq_wake_due(&task, IRQ | TIMER));
        assert!(
            !irq_wake_due(&task, TIMER),
            "a mask without IRQ ignores lines"
        );
        assert_eq!(
            publish_wait_state_locked(&mut task, TID + 1, IRQ | TIMER, Some(5)),
            CompletionParkDecision::IrqPending
        );
        assert_eq!(task.state, TaskState::Ready, "the waiter does not park");
        assert!(irq_wait::take_pending(0xC4) && irq_wait::take_pending(0xC5));
        let _ = queue.release(slot);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn an_irq_wait_without_lines_is_refused_before_reserving() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        assert!(source_is_valid(IRQ, None), "x86_64 serves IRQ");
        assert!(source_is_valid(IRQ | TIMER, Some(1)));
        assert!(!source_is_valid(1 << 4, Some(1)));
        assert_eq!(
            wait_completion(72_302, IRQ | TIMER, Some(1), 0x1000),
            Err(SyscallError::PermissionDenied)
        );
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn a_take_after_revoke_fails_and_an_empty_refresh_releases_the_slot() {
        use crate::dev::pcie_msi;
        const OWNER: usize = 72_301;
        let _guard = crate::TEST_STATE_LOCK.lock();
        let queue = queue();
        let revoked = pcie_msi::allocate_for_test(OWNER, 0x0020, 2);
        let slot = queue.reserve().unwrap();
        irq_wait::restore_pending(revoked.first_irq);
        assert_eq!(pcie_msi::release_for(OWNER), 1);
        assert!(!take_owned_line(OWNER, revoked.first_irq));
        assert_eq!(
            irq_lines_after_revoke(OWNER, &queue, slot, false),
            Err(SyscallError::PermissionDenied)
        );
        assert!(settled(&queue), "the waiter-owned slot is released");

        let kept = pcie_msi::allocate_for_test(OWNER, 0x0028, 1);
        let slot = queue.reserve().unwrap();
        let lines = irq_lines_after_revoke(OWNER, &queue, slot, false).expect("a line left");
        assert!(lines.contains(kept.first_irq));
        assert_eq!(queue.reserved(), 1, "the wait keeps its reservation");
        let _ = queue.release(slot);
        assert_eq!(pcie_msi::release_for(OWNER), 1);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn a_failed_copy_out_re_pends_the_irq_line() {
        use crate::dev::pcie_msi;
        const OWNER: usize = 72_303;
        let _guard = crate::TEST_STATE_LOCK.lock();
        let queue = queue();
        let line = pcie_msi::allocate_for_test(OWNER, 0x0030, 1).first_irq;
        let slot = queue.reserve().unwrap();
        irq_wait::restore_pending(line);
        assert!(take_owned_line(OWNER, line));
        let selection = Selection {
            source: IRQ,
            result: line as isize,
            relevel_net_rx: false,
            scanned: 0,
        };
        // OWNER is not a live task, so the copy-out fails.
        assert!(finish_multi(OWNER, 0x1000, &queue, slot, false, selection).is_err());
        assert!(settled(&queue));
        assert!(take_owned_line(OWNER, line), "the line is pending again");
        assert_eq!(pcie_msi::release_for(OWNER), 1);
    }
}

/// Write `done` into the caller's buffer.
///
/// Precondition: `out_ptr` has been through `validate_user_buf` for
/// [`COMPLETION_LEN`] bytes in this call.
fn write_completion(
    caller_id: usize,
    out_ptr: usize,
    done: Completion,
) -> Result<usize, SyscallError> {
    let record = ViCompletion {
        slot: done.slot.index() as u32,
        source: done.source,
        result: done.result as i64,
    }
    .to_bytes();
    let view =
        super::copy_glue::TaskCopyView::for_task(caller_id).ok_or(SyscallError::InvalidInput)?;
    view.write_bytes(out_ptr, &record)
        .map_err(|_| SyscallError::InvalidInput)?;
    Ok(1)
}
