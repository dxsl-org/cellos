//! Boot self-test for the per-cell completion queue.
//!
//! Nothing is migrated onto the queue yet, so it has no production caller and no
//! test would otherwise touch it. The four properties proven here are the ones
//! that stop being cheap to fix once something depends on them:
//!
//! - a reserved slot round-trips a result, and releases on drain;
//! - threads of one cell share one queue, so the bound is per cell rather than
//!   per thread;
//! - a full queue refuses the *submission* and still lands every completion it
//!   already promised — the opposite failure, dropping a completion, is a hang
//!   at the point furthest from its cause;
//! - an append reaches the scheduler through the deferred wake rather than
//!   inline, which is the only shape that is safe from interrupt context.
//!
//! Runs in the same single-hart window as the other task self-tests — after
//! `task::init()` and before `smp::start_secondaries()`. The synthetic tasks are
//! inserted into the task table but never pushed onto a ready queue, so
//! `pick_next` cannot switch to one; the wake row, which does push, holds
//! `SCHEDULER` for its whole length so no tick can observe the intermediate
//! state. The scheduler is left exactly as it was found.

use super::completion::{self, CompletionQueue, QUEUE_CAPACITY};
use super::tcb::{Task, TaskState};
use alloc::sync::Arc;
use types::CellId;

/// Synthetic tids and cells outside anything the boot sequence has assigned.
const TID_A: usize = 9301;
const TID_B: usize = 9302;
const TID_C: usize = 9303;
const CELL_ONE: u64 = 9401;
const CELL_TWO: u64 = 9402;

/// Put a synthetic task in the table. Shared with the NET_RX self-test, which
/// needs the same scaffolding to reach a real queue.
pub(super) fn insert(tid: usize, cell: u64) {
    let task = alloc::boxed::Box::new(Task::new(
        tid,
        CellId(cell),
        "cq-selftest",
        alloc::vec::Vec::new(),
    ));
    if let Some(sched) = super::SCHEDULER.lock().as_mut() {
        sched.tasks.insert(tid, task);
    }
}

pub(super) fn remove(tid: usize) {
    if let Some(sched) = super::SCHEDULER.lock().as_mut() {
        sched.tasks.remove(&tid);
    }
    super::hart_local::ready::remove_from_all(tid);
}

pub(super) fn queue(tid: usize) -> Option<Arc<CompletionQueue>> {
    let mut guard = super::SCHEDULER.lock();
    completion::queue_for(guard.as_mut()?, tid)
}

fn fail(reason: &str) -> bool {
    log::error!("[selftest] COMPLETION-QUEUE: FAIL — {}", reason);
    false
}

/// A result lands in the slot that was reserved for it, and the slot is free
/// again once drained.
fn round_trip() -> bool {
    insert(TID_A, CELL_ONE);
    let mut ok = true;
    match queue(TID_A) {
        Some(q) => match q.reserve() {
            Some(slot) => {
                if !q.complete(slot, -7) {
                    ok = fail("completing a freshly reserved slot was refused");
                }
                match q.drain() {
                    Some(done) if done.slot == slot && done.result == -7 => {}
                    other => {
                        ok = fail(&alloc::format!(
                            "drained {:?}, expected the slot back with -7",
                            other
                        ))
                    }
                }
                if q.drain().is_some() {
                    ok = fail("a second drain produced a completion that was never appended");
                }
                if q.reserved() != 0 || q.drainable() != 0 {
                    ok = fail("draining left the slot charged");
                }
            }
            None => ok = fail("an empty queue refused the first reservation"),
        },
        None => ok = fail("no queue could be reached from the task record"),
    }
    remove(TID_A);
    ok
}

/// The queue is per cell: a second thread of the same cell gets the same one, a
/// task in another cell does not.
fn shared_within_cell() -> bool {
    insert(TID_A, CELL_ONE);
    insert(TID_B, CELL_ONE);
    insert(TID_C, CELL_TWO);

    let mut ok = true;
    match (queue(TID_A), queue(TID_B), queue(TID_C)) {
        (Some(a), Some(b), Some(c)) => {
            if !Arc::ptr_eq(&a, &b) {
                ok = fail("two threads of one cell were given separate queues");
            }
            if Arc::ptr_eq(&a, &c) {
                ok = fail("two cells were given the same queue");
            }
            if a.cell() != CellId(CELL_ONE) || c.cell() != CellId(CELL_TWO) {
                ok = fail("a queue reports the wrong owning cell");
            }
        }
        _ => ok = fail("a live task could not reach a queue"),
    }

    remove(TID_A);
    remove(TID_B);
    remove(TID_C);
    ok
}

/// A full queue refuses the next submission, and every completion it already
/// promised still lands.
fn exhaustion_refuses_submission() -> bool {
    insert(TID_A, CELL_ONE);
    let mut ok = true;
    match queue(TID_A) {
        Some(q) => {
            let mut slots = alloc::vec::Vec::with_capacity(QUEUE_CAPACITY);
            for _ in 0..QUEUE_CAPACITY {
                match q.reserve() {
                    Some(slot) => slots.push(slot),
                    None => {
                        ok = fail("the queue refused a reservation below its capacity");
                        break;
                    }
                }
            }
            if q.reserve().is_some() {
                ok = fail("a full queue accepted another submission");
            }
            // The point of reserving at submission: refusal above did not cost
            // any of the operations already in flight their landing place.
            for (n, slot) in slots.iter().enumerate() {
                if !q.complete(*slot, n as isize) {
                    ok = fail("a completion was refused while the queue was full");
                }
            }
            if q.drainable() != QUEUE_CAPACITY {
                ok = fail("a completion was dropped instead of stored");
            }
            for n in 0..QUEUE_CAPACITY {
                match q.drain() {
                    Some(done) if done.result == n as isize => {}
                    _ => {
                        ok = fail("completions did not drain in submission order");
                        break;
                    }
                }
            }
            match q.reserve() {
                // Completed and drained so the queue is left as it was found,
                // rather than with a reservation nothing will ever land in.
                Some(slot) => {
                    if !q.complete(slot, 0) || q.drain().is_none() {
                        ok = fail("a drained queue no longer round-trips");
                    }
                }
                None => ok = fail("a drained queue still refuses submissions"),
            }
        }
        None => ok = fail("no queue could be reached from the task record"),
    }
    remove(TID_A);
    ok
}

/// An append does not wake inline; the flag it raises becomes a scheduler wake
/// only when the deferred step runs.
fn deferred_wake_reaches_scheduler() -> bool {
    insert(TID_A, CELL_ONE);
    let q = match queue(TID_A) {
        Some(q) => q,
        None => {
            remove(TID_A);
            return fail("no queue could be reached from the task record");
        }
    };
    q.register_waiter(TID_A);

    // Held across append, delivery and teardown: the guard keeps interrupts off,
    // so no timer tick can run the real deferred wake and make a synthetic task
    // with no context runnable behind this test's back.
    let mut ok = true;
    {
        let mut guard = super::SCHEDULER.lock();
        let sched = match guard.as_mut() {
            Some(sched) => sched,
            None => {
                drop(guard);
                remove(TID_A);
                return fail("scheduler absent");
            }
        };
        if let Some(task) = sched.tasks.get_mut(&TID_A) {
            task.state = TaskState::Sleeping { until: usize::MAX };
        }
        // Start from a cleared flag, so the one observed after the append below
        // is provably this append's and not a leftover from an earlier row.
        completion::deliver_pending_wakes(sched);
        if completion::wakes_pending() {
            ok = fail("a wake request survived delivery before any append");
        }

        let slot = match q.reserve() {
            Some(slot) => slot,
            None => {
                drop(guard);
                remove(TID_A);
                return fail("an empty queue refused a reservation");
            }
        };
        let before = super::hart_local::ready::total_ready_count();
        if !q.complete(slot, 1) {
            ok = fail("completing a freshly reserved slot was refused");
        }
        if super::hart_local::ready::total_ready_count() != before {
            ok = fail("the append woke the waiter inline instead of deferring it");
        }
        if !completion::wakes_pending() {
            ok = fail("the append raised no wake request");
        }

        completion::deliver_pending_wakes(sched);

        if super::hart_local::ready::total_ready_count() != before + 1 {
            ok = fail("the deferred wake did not reach the scheduler");
        }
        if sched.tasks.get(&TID_A).map(|t| &t.state) != Some(&TaskState::Ready) {
            ok = fail("the woken task was left parked");
        }
        if completion::wakes_pending() {
            ok = fail("delivery left the wake request raised");
        }
        sched.tasks.remove(&TID_A);
    }
    super::hart_local::ready::remove_from_all(TID_A);
    let _ = q.clear_waiter(TID_A);
    ok
}

/// Cleanup is ownership-aware: an older waiter must not clear the task that
/// replaced it on the cell's shared queue.
fn waiter_cleanup_preserves_replacement() -> bool {
    insert(TID_A, CELL_ONE);
    insert(TID_B, CELL_ONE);
    let q = match queue(TID_A) {
        Some(q) => q,
        None => {
            remove(TID_A);
            remove(TID_B);
            return fail("no queue could be reached from the task record");
        }
    };

    q.register_waiter(TID_A);
    q.register_waiter(TID_B);
    let mut ok = true;
    if q.clear_waiter(TID_A) {
        ok = fail("an older waiter erased its replacement");
    }
    if !q.clear_waiter(TID_B) {
        ok = fail("the current waiter could not clear its registration");
    }

    remove(TID_A);
    remove(TID_B);
    ok
}

/// Draining while still running consumes the associated wake request, so it
/// cannot cancel the task's next unrelated park.
fn self_drain_cancels_stale_wake() -> bool {
    insert(TID_A, CELL_ONE);
    let q = match queue(TID_A) {
        Some(q) => q,
        None => {
            remove(TID_A);
            return fail("no queue could be reached from the task record");
        }
    };
    q.register_waiter(TID_A);
    let mut ok = true;
    match q.reserve() {
        Some(slot) if q.complete(slot, 1) && q.drain().is_some() => {}
        _ => ok = fail("a self-drained completion did not round-trip"),
    }
    {
        let mut guard = super::SCHEDULER.lock();
        if let Some(sched) = guard.as_mut() {
            if let Some(task) = sched.tasks.get_mut(&TID_A) {
                task.state = TaskState::Sleeping { until: usize::MAX };
            }
            completion::deliver_pending_wakes(sched);
            if sched.tasks.get(&TID_A).map(|task| &task.state)
                != Some(&TaskState::Sleeping { until: usize::MAX })
            {
                ok = fail("a self-drained completion woke the task's next park");
            }
        }
    }
    let _ = q.clear_waiter(TID_A);
    remove(TID_A);
    ok
}

/// Withdrawing a reservation frees the slot and raises no wake request.
///
/// A withdrawal expressed as a completion would leave a request outstanding for
/// a task that is running, and that request cancels the submitter's *next* park
/// the instant it begins — a submit/withdraw loop would then never sleep.
fn withdrawal_raises_no_wake() -> bool {
    insert(TID_A, CELL_ONE);
    let mut ok = true;
    match queue(TID_A) {
        Some(q) => {
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                completion::deliver_pending_wakes(sched);
            }
            match q.reserve() {
                Some(slot) => {
                    if !q.release(slot) {
                        ok = fail("withdrawing a freshly reserved slot was refused");
                    }
                    if completion::wakes_pending() {
                        ok = fail("a withdrawal raised a wake request");
                    }
                    if q.drainable() != 0 {
                        ok = fail("a withdrawal left something to drain");
                    }
                    if q.reserved() != 0 {
                        ok = fail("a withdrawal left the slot charged");
                    }
                    if q.release(slot) {
                        ok = fail("a free slot was withdrawn a second time");
                    }
                }
                None => ok = fail("an empty queue refused the first reservation"),
            }
            // A slot holding a result must be drained, never discarded.
            if let Some(slot) = q.reserve() {
                if !q.complete(slot, 5) {
                    ok = fail("completing a freshly reserved slot was refused");
                }
                if q.release(slot) {
                    ok = fail("a slot holding a result was withdrawn instead of drained");
                }
                if q.drain().is_none() {
                    ok = fail("the refused withdrawal lost the result it protected");
                }
            }
            if let Some(sched) = super::SCHEDULER.lock().as_mut() {
                completion::deliver_pending_wakes(sched);
            }
        }
        None => ok = fail("no queue could be reached from the task record"),
    }
    remove(TID_A);
    ok
}

/// Returns true iff the completion queue reserves, lands, bounds and defers as
/// specified. Logs a decisive serial line.
pub fn self_test() -> bool {
    let ok = round_trip()
        & shared_within_cell()
        & exhaustion_refuses_submission()
        & deferred_wake_reaches_scheduler()
        & waiter_cleanup_preserves_replacement()
        & self_drain_cancels_stale_wake()
        & withdrawal_raises_no_wake();

    if ok {
        log::info!(
            "[selftest] COMPLETION-QUEUE: PASS (cap {} slots, {} bytes per cell)",
            QUEUE_CAPACITY,
            core::mem::size_of::<CompletionQueue>()
        );
    } else {
        log::error!("[selftest] COMPLETION-QUEUE: FAIL");
    }
    ok
}
