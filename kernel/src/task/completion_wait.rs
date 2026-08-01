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
use super::tcb::TaskState;
use super::waker;
use api::completion::{ViCompletion, COMPLETION_LEN};

/// Removes this waiter's registration on every return path without erasing a
/// newer waiter that may have replaced it on the shared per-cell queue.
struct WaiterRegistration<'a> {
    queue: &'a CompletionQueue,
    tid: usize,
}

impl<'a> WaiterRegistration<'a> {
    fn new(queue: &'a CompletionQueue, tid: usize) -> Self {
        queue.register_waiter(tid);
        Self { queue, tid }
    }
}

impl Drop for WaiterRegistration<'_> {
    fn drop(&mut self) {
        let _ = self.queue.clear_waiter(self.tid);
    }
}

/// Reserve a slot on the caller's completion queue, wait for it to be filled,
/// and write the result to `out_ptr`.
///
/// `mask` names exactly one source; `deadline` is an absolute tick count, or
/// `None` to wait indefinitely.
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
    // One bit only: a submission is made against one source, and a mask naming
    // two would leave the second one with no reservation of its own.
    if mask != api::syscall::events::NET_RX {
        return Err(SyscallError::InvalidInput);
    }
    validate_user_buf(out_ptr, COMPLETION_LEN, COMPLETION_LEN)?;

    let queue = {
        let mut guard = super::SCHEDULER.lock();
        let sched = guard.as_mut().ok_or(SyscallError::Unknown)?;
        completion::queue_for(sched, caller_id).ok_or(SyscallError::Unknown)?
    };
    let slot = queue.reserve().ok_or(SyscallError::TryAgain)?;
    let _waiter = WaiterRegistration::new(&queue, caller_id);
    waker::arm_net_rx(queue.clone(), slot);

    // Lost-wakeup guard, first half: a frame that arrived while nobody held a
    // reservation is remembered in the level flag. Consuming it only *after* the
    // arm is what makes the check total — a frame arriving during the check has
    // a reservation to complete instead.
    if waker::consume_pending(mask) != 0
        && matches!(waker::disarm_net_rx(&queue), waker::DisarmResult::Owned(_))
    {
        // The result is reported straight out of the reservation rather than
        // pushed through the queue and pulled back: appending would raise a
        // wake request for a task that is not parked, and that request would
        // still be outstanding to cancel the *next* park.
        if queue.release(slot) {
            return Ok(write_completion(
                out_ptr,
                Completion {
                    slot,
                    result: mask as isize,
                },
            ));
        }
    }

    // Second half: anything the source landed while the steps above ran is
    // delivered without parking at all.
    if let Some(done) = queue.drain() {
        return Ok(write_completion(out_ptr, done));
    }

    // `WaitEvent` with no event bits: this waiter's result comes from the queue,
    // so the sweep must not consume a fired bit on its behalf and report an
    // empty wake. The deadline is the one thing the sweep is still asked for.
    {
        let mut guard = super::SCHEDULER.lock();
        if let Some(task) = guard.as_mut().and_then(|s| s.tasks.get_mut(&caller_id)) {
            task.state = TaskState::WaitEvent { mask: 0, deadline };
        }
    }
    super::yield_cpu();

    if let Some(done) = queue.drain() {
        return Ok(write_completion(out_ptr, done));
    }

    // Nothing landed: the deadline passed, or the wake was spurious. Either way
    // the reservation goes back, and the race with a frame arriving at this
    // exact moment is settled by whoever takes the slot.
    loop {
        match waker::disarm_net_rx(&queue) {
            waker::DisarmResult::Owned(own) if queue.release(own) => return Ok(0),
            // The source owns the slot but has not published it yet. This can
            // only run concurrently on another hart, so wait until the queue
            // record becomes visible before clearing the waiter registration.
            waker::DisarmResult::Completing => core::hint::spin_loop(),
            // Either the source completed the slot as it was being withdrawn,
            // or another waiter displaced it. Both publish a result before
            // leaving the Completing state.
            _ => {
                return match queue.drain() {
                    Some(done) => Ok(write_completion(out_ptr, done)),
                    None => Ok(0),
                };
            }
        }
        if let Some(done) = queue.drain() {
            return Ok(write_completion(out_ptr, done));
        }
    }
}

/// Write `done` into the caller's buffer.
///
/// Precondition: `out_ptr` has been through `validate_user_buf` for
/// [`COMPLETION_LEN`] bytes in this call.
fn write_completion(out_ptr: usize, done: Completion) -> usize {
    let record = ViCompletion {
        slot: done.slot.index() as u32,
        result: done.result as i64,
    }
    .to_bytes();
    // SAFETY: `out_ptr` is a non-null caller buffer validated for exactly
    // COMPLETION_LEN bytes at entry to `wait_completion`, and `record` is a
    // distinct kernel stack array of that same length, so the two cannot
    // overlap. A byte copy has no alignment requirement.
    unsafe { core::ptr::copy_nonoverlapping(record.as_ptr(), out_ptr as *mut u8, COMPLETION_LEN) };
    1
}
