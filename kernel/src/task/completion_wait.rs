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
use api::completion::{ViCompletion, COMPLETION_LEN};

/// Removes this waiter's registration on every return path without erasing a
/// newer waiter that may have replaced it on the shared per-cell queue.
pub(super) struct WaiterRegistration<'a> {
    queue: &'a CompletionQueue,
    tid: usize,
    slot: super::completion::SlotId,
}

impl<'a> WaiterRegistration<'a> {
    pub(super) fn new(
        queue: &'a CompletionQueue,
        tid: usize,
        slot: super::completion::SlotId,
        source: u32,
    ) -> Option<Self> {
        queue.register_waiter(tid);
        let registered = {
            let mut guard = super::SCHEDULER.lock();
            guard
                .as_mut()
                .and_then(|sched| sched.tasks.get_mut(&tid))
                .map(|task| {
                    task.completion_wait = Some(CompletionWait { source, slot });
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
pub(super) fn source_is_valid(mask: u32, deadline: Option<u64>) -> bool {
    api::completion::source::is_single_supported(mask)
        && (mask != api::completion::source::TIMER || deadline.is_some())
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum CompletionParkDecision {
    Parked,
    MailboxPending,
}

/// Publish a completion wait only if no already-published IPC must interrupt it.
///
/// The caller holds `SCHEDULER`; keeping the mailbox predicate, outgoing
/// context handoff, and state write in this one critical section closes both
/// enqueue-before-park orderings and the state-before-yield SMP window. TIMER
/// is deliberately deadline-only and therefore ignores the mailbox.
pub(super) fn publish_wait_state_locked(
    task: &mut Task,
    tid: usize,
    source: u32,
    deadline: Option<u64>,
) -> CompletionParkDecision {
    if source == api::completion::source::NET_RX && !task.pending_msgs.is_empty() {
        return CompletionParkDecision::MailboxPending;
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
/// `mask` names exactly one source. `deadline` is an absolute tick count;
/// `None` is valid only for an indefinite `NET_RX` wait because `TIMER` needs a
/// finite instant to complete against.
///
/// # Returns
/// `1` when a completion was written to `out_ptr`, `0` when the wait ended with
/// nothing to report.
///
/// # Errors
/// - `InvalidInput` — `mask` does not name exactly one source this kernel
///   serves, or `out_ptr` is null.
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
    // One known bit only: a submission is made against one source, and a mask
    // naming two would leave the second one with no reservation of its own.
    if !source_is_valid(mask, deadline) {
        return Err(SyscallError::InvalidInput);
    }
    validate_user_buf(out_ptr, COMPLETION_LEN, COMPLETION_LEN)?;

    let queue = {
        let mut guard = super::SCHEDULER.lock();
        let sched = guard.as_mut().ok_or(SyscallError::Unknown)?;
        completion::queue_for(sched, caller_id).ok_or(SyscallError::Unknown)?
    };
    let slot = queue.reserve().ok_or(SyscallError::TryAgain)?;
    let _waiter = match WaiterRegistration::new(&queue, caller_id, slot, mask) {
        Some(waiter) => waiter,
        None => {
            let _ = queue.release(slot);
            return Err(SyscallError::Unknown);
        }
    };
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
