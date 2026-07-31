//! Per-cell completion queue: the kernel-owned place an asynchronous result lands.
//!
//! The queue is heap memory the kernel allocates and holds through the task
//! record. It is deliberately **not** a grant: a grant can be unregistered or
//! freed by the cell that owns it, and a queue that can vanish under an
//! in-flight operation turns every completion into a write to whatever was
//! handed the frame next. Because the queue is kernel-owned, appending needs no
//! address resolution and no allocator, which is what lets the append path hold
//! one leaf lock and nothing else.
//!
//! Two rules make a completion infallible:
//!
//! - **A slot is reserved when the operation is submitted**, from the submitting
//!   context, where a refusal is an ordinary error the caller can act on. By the
//!   time anything is in flight its landing place already exists.
//! - **Appending never allocates and never grows the queue.** The drainable ring
//!   holds slot indexes and is exactly as long as the slot array, so it cannot
//!   overflow: a slot contributes at most one entry between reservation and
//!   release.
//!
//! Waking the task that is waiting is a *separate*, deferred step. An append may
//! run in interrupt context, and making a task runnable needs the scheduler lock,
//! which must never be taken from there. So an append only raises a flag; the
//! flag is turned into a scheduler wake by [`deliver_pending_wakes`], called from
//! `yield_cpu` after `SCHEDULER` is free — the same deferral the grant reap uses
//! for work that cannot run under the sweep's locks.
//!
//! Lock order: this module's leaf lock is taken alone. It is never held across a
//! call that takes `SCHEDULER`, `FRAME_ALLOCATOR` or `KERNEL_ROOT`, and none of
//! those is held when it is taken.

use super::scheduler::Scheduler;
use super::tcb::TaskState;
use crate::sync::Spinlock;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use types::CellId;

/// Operations one cell may have outstanding at once.
///
/// The cap is the backpressure: a cell that submits without draining is refused
/// at submission rather than served, so the failure is visible, attributable and
/// confined to the cell that caused it.
pub const QUEUE_CAPACITY: usize = 32;

/// Names one submission within the cell that reserved it.
///
/// A slot — not a task id — is the identifier a completion carries. Task ids are
/// reused, do not survive a restart, and cannot tell two operations of one task
/// apart. A slot is meaningful only inside the cell that owns the queue and dies
/// with it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SlotId(u16);

impl SlotId {
    /// Position of this slot in its queue. Always `< QUEUE_CAPACITY`.
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// A finished operation, ready to be handed to whoever was waiting.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Completion {
    pub slot: SlotId,
    /// Operation result. Negative values are reserved for errors; the exact
    /// encoding belongs with the first operation migrated onto the queue, not
    /// here, so nothing is frozen before a real caller exists.
    pub result: isize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Slot {
    /// No submission owns this index.
    Free,
    /// A submission owns this index; its result has not arrived.
    Reserved,
    /// The result arrived and has not been drained.
    Done(isize),
}

struct Ring {
    slots: [Slot; QUEUE_CAPACITY],
    /// Indexes of slots holding an undrained result, oldest first.
    drainable: [u16; QUEUE_CAPACITY],
    head: usize,
    len: usize,
}

/// Raised by any append, cleared by [`deliver_pending_wakes`]. A single relaxed
/// load keeps the common tick — no completions anywhere — at one atomic read.
static WAKES_PENDING: AtomicBool = AtomicBool::new(false);

/// The landing ground for one cell's completions.
///
/// Shared by every thread of the cell through `Arc`, so the last reference to
/// disappear frees it. A completion source that still holds a reference keeps
/// the queue alive past the cell's death: the append then lands in a queue
/// nobody drains, which leaks a bounded allocation rather than corrupting
/// whatever took the memory next.
pub struct CompletionQueue {
    cell: CellId,
    ring: Spinlock<Ring>,
    /// Task to make runnable when a completion arrives; 0 means nobody is
    /// waiting. See [`CompletionQueue::register_waiter`] for the contract.
    waiter: AtomicUsize,
    wake_requested: AtomicBool,
}

impl CompletionQueue {
    fn new(cell: CellId) -> Self {
        Self {
            cell,
            ring: Spinlock::new(Ring {
                slots: [Slot::Free; QUEUE_CAPACITY],
                drainable: [0; QUEUE_CAPACITY],
                head: 0,
                len: 0,
            }),
            waiter: AtomicUsize::new(0),
            wake_requested: AtomicBool::new(false),
        }
    }

    /// Cell this queue belongs to.
    pub fn cell(&self) -> CellId {
        self.cell
    }

    /// Take a slot for an operation about to be submitted.
    ///
    /// # Errors
    /// `None` when every slot is taken. Call this from the submitting cell's own
    /// context and turn it into an ordinary error there: refusing an interrupt
    /// handler that is trying to report completion would deadlock the system it
    /// was serving, so the refusal has to happen here, before anything is in
    /// flight.
    pub fn reserve(&self) -> Option<SlotId> {
        let mut ring = self.ring.lock();
        let index = ring.slots.iter().position(|s| *s == Slot::Free)?;
        ring.slots[index] = Slot::Reserved;
        Some(SlotId(index as u16))
    }

    /// Record the result of the operation holding `slot`.
    ///
    /// Takes this queue's leaf lock and nothing else, so it is safe from
    /// interrupt context. Waking the waiter is deliberately not done here — see
    /// [`deliver_pending_wakes`].
    ///
    /// # Returns
    /// `true` once the result is stored. `false` only for a kernel-side protocol
    /// violation — completing a slot that was never reserved, or completing one
    /// twice — which is logged and leaves the queue untouched. A slot obtained
    /// from [`reserve`](Self::reserve) and completed once always succeeds, which
    /// is the property the whole design rests on.
    #[must_use = "a refused completion means a slot was misused and the waiter will hang"]
    pub fn complete(&self, slot: SlotId, result: isize) -> bool {
        let index = slot.index();
        // The whole lock-holding region is this block, and it contains no call
        // that could reach another lock — not the logger, which is why the
        // refusal below reports after the guard is dropped.
        let refused = {
            let mut ring = self.ring.lock();
            match ring.slots.get(index) {
                Some(Slot::Reserved) => {
                    ring.slots[index] = Slot::Done(result);
                    // Cannot overflow: `drainable` is as long as `slots`, and a
                    // slot is only pushed on the Reserved -> Done edge, which
                    // happens once per reservation.
                    let position = (ring.head + ring.len) % QUEUE_CAPACITY;
                    ring.drainable[position] = index as u16;
                    ring.len += 1;
                    None
                }
                Some(Slot::Free) => Some("free"),
                Some(Slot::Done(_)) => Some("already done"),
                None => Some("out of range"),
            }
        };

        if let Some(why) = refused {
            log::error!(
                "[completion] cell {} slot {} completed while {}",
                self.cell.0,
                index,
                why
            );
            return false;
        }

        // Flagged only after the entry is visible, so a drain triggered by the
        // flag can never observe an empty queue and go back to sleep.
        self.wake_requested.store(true, Ordering::Release);
        WAKES_PENDING.store(true, Ordering::Release);
        true
    }

    /// Withdraw a reservation whose operation never started.
    ///
    /// This is not a completion and must never be expressed as one: completing
    /// a slot raises a wake request, and a request raised by the submitter
    /// itself — which is already running — is still outstanding when that same
    /// task parks next, so the park is cancelled the instant it begins. A
    /// submitter that withdraws and re-submits in a loop would then never
    /// sleep at all.
    ///
    /// # Returns
    /// `true` when the slot was reserved and is now free. `false` when it
    /// already holds a result: the submitter lost the race with the source and
    /// must [`drain`](Self::drain) that result rather than discard it.
    #[must_use = "a refused withdrawal means a result is waiting and would be lost"]
    pub fn release(&self, slot: SlotId) -> bool {
        let mut ring = self.ring.lock();
        match ring.slots.get(slot.index()) {
            Some(Slot::Reserved) => {
                ring.slots[slot.index()] = Slot::Free;
                true
            }
            _ => false,
        }
    }

    /// Take the oldest undrained completion, releasing its slot for reuse.
    ///
    /// Holds the same single leaf lock as [`complete`](Self::complete); the
    /// report of an inconsistency is deliberately made after the guard drops.
    pub fn drain(&self) -> Option<Completion> {
        let taken = {
            let mut ring = self.ring.lock();
            if ring.len == 0 {
                return None;
            }
            let index = ring.drainable[ring.head] as usize;
            ring.head = (ring.head + 1) % QUEUE_CAPACITY;
            ring.len -= 1;
            match ring.slots[index] {
                Slot::Done(result) => {
                    ring.slots[index] = Slot::Free;
                    Ok(Completion {
                        slot: SlotId(index as u16),
                        result,
                    })
                }
                // Unreachable while the ring and the slot array agree; treat a
                // disagreement as an empty drain rather than a stale read.
                _ => Err(index),
            }
        };

        match taken {
            Ok(completion) => Some(completion),
            Err(index) => {
                log::error!(
                    "[completion] cell {} drained slot {} that holds no result",
                    self.cell.0,
                    index
                );
                None
            }
        }
    }

    /// Slots taken by submissions that have not yet reported a result.
    pub fn reserved(&self) -> usize {
        let ring = self.ring.lock();
        ring.slots.iter().filter(|s| **s == Slot::Reserved).count()
    }

    /// Completions recorded and not yet drained.
    pub fn drainable(&self) -> usize {
        self.ring.lock().len
    }

    /// Name `tid` as the task to make runnable when a completion arrives.
    ///
    /// # Contract
    /// The registrant must be parked in a state whose only wake condition is
    /// this queue. Registering a task that is also parked on IPC or a timer
    /// makes it runnable early, and the state it parked in is what those paths
    /// match on. Registration is kernel-internal and has no caller outside the
    /// boot self-test; the park state belongs with the first operation migrated
    /// onto the queue, where a real caller can pin its shape.
    pub fn register_waiter(&self, tid: usize) {
        self.waiter.store(tid, Ordering::Release);
    }

    /// Stop waking anyone for this queue.
    pub fn clear_waiter(&self) {
        self.waiter.store(0, Ordering::Release);
    }

    /// Consume a pending wake request, yielding the task to make runnable.
    fn take_wake_request(&self) -> Option<usize> {
        if !self.wake_requested.swap(false, Ordering::AcqRel) {
            return None;
        }
        match self.waiter.load(Ordering::Acquire) {
            0 => None,
            tid => Some(tid),
        }
    }
}

/// The queue serving `tid`'s cell, created on first use.
///
/// Threads of one cell share one queue, so the handle is looked up across the
/// cell before a new one is allocated. Returns `None` only when `tid` names no
/// live task.
///
/// Call from a submitting context with `SCHEDULER` held. The append path never
/// comes here — it uses the handle it was given, which is the reason it needs no
/// lock but its own.
pub fn queue_for(sched: &mut Scheduler, tid: usize) -> Option<Arc<CompletionQueue>> {
    let cell = sched.tasks.get(&tid)?.cell_id;
    if let Some(existing) = sched.tasks.get(&tid).and_then(|t| t.completion.clone()) {
        return Some(existing);
    }
    let queue = sched
        .tasks
        .values()
        .filter(|t| t.cell_id == cell)
        .find_map(|t| t.completion.clone())
        .unwrap_or_else(|| Arc::new(CompletionQueue::new(cell)));
    sched.tasks.get_mut(&tid)?.completion = Some(queue.clone());
    Some(queue)
}

/// Whether any append has asked for a wake since the last delivery.
pub fn wakes_pending() -> bool {
    WAKES_PENDING.load(Ordering::Acquire)
}

/// A state a completion wake may leave, versus one it must not disturb.
///
/// `Ready`/`Running` are already runnable, a `Terminated` task has nowhere to go,
/// and `Frozen` is held deliberately by hot-swap — unfreezing it here would run a
/// cell mid-swap.
fn is_parked(state: &TaskState) -> bool {
    !matches!(
        state,
        TaskState::Ready | TaskState::Running | TaskState::Terminated | TaskState::Frozen { .. }
    )
}

/// Turn flagged completions into scheduler wakes.
///
/// Runs where `SCHEDULER` may be taken and neither `FRAME_ALLOCATOR` nor
/// `KERNEL_ROOT` is held — that is, from `yield_cpu` and not from an append.
/// Requires `SCHEDULER` to be held by the caller.
pub fn deliver_pending_wakes(sched: &mut Scheduler) {
    // Cleared before the scan: an append landing during the scan re-raises the
    // flag, so at worst its wake waits for the next tick. Clearing afterwards
    // would swallow it entirely.
    if !WAKES_PENDING.swap(false, Ordering::AcqRel) {
        return;
    }

    // Reached through the task table, so a queue kept alive only by a completion
    // source outlives its cell unreachable — which is the intended end state: its
    // registered waiter is dead, and the alternative, a wake for a tid the table
    // has since reissued, would make a stranger runnable.
    let mut requested: Vec<usize> = Vec::new();
    for task in sched.tasks.values() {
        if let Some(queue) = task.completion.as_ref() {
            if let Some(tid) = queue.take_wake_request() {
                requested.push(tid);
            }
        }
    }

    let mut runnable: Vec<usize> = Vec::new();
    for tid in requested {
        if let Some(task) = sched.tasks.get_mut(&tid) {
            if is_parked(&task.state) {
                task.state = TaskState::Ready;
                runnable.push(tid);
            }
        }
    }
    for tid in runnable {
        sched.push_ready(tid);
    }
}
