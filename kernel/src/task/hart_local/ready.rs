//! Per-hart ready-queue helpers for the Phase 03 work-stealing scheduler.
//!
//! Lock order: SCHEDULER (global, coarse) → per-hart ready lock (leaf).
//! `steal_from_busiest` holds only leaf locks — never SCHEDULER.
//!
//! RT tasks (priority ≥ RealTime) are never stolen; Phase 04 will hart-pin them.

use super::{HART_LOCALS, MAX_HARTS};
use alloc::collections::{BTreeMap, VecDeque};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering as PromiseOrdering};

#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
use core::sync::atomic::{AtomicUsize, Ordering};

/// Test-only dispatch reservation. The context-handoff regression uses this
/// to keep its Normal-priority worker on hart 1 until that worker has begun
/// the controlled block-before-yield handoff.
#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
static TEST_DISPATCH_RESERVATIONS: [AtomicUsize; MAX_HARTS] = {
    #[allow(clippy::declare_interior_mutable_const)]
    const EMPTY: AtomicUsize = AtomicUsize::new(0);
    [EMPTY; MAX_HARTS]
};

/// Reserve a test worker's initial dispatch on `hart_id`.
///
/// The reservation affects only work stealing; local selection on the
/// reserved hart remains normal scheduler behavior.
#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
pub fn reserve_test_dispatch_on_hart(hart_id: usize, task_id: usize) -> bool {
    task_id != 0
        && hart_id < MAX_HARTS
        && TEST_DISPATCH_RESERVATIONS[hart_id]
            .compare_exchange(0, task_id, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
}

/// Release a test worker's initial-dispatch reservation.
#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
pub fn release_test_dispatch_on_hart(hart_id: usize, task_id: usize) -> bool {
    hart_id < MAX_HARTS
        && TEST_DISPATCH_RESERVATIONS[hart_id]
            .compare_exchange(task_id, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
}

/// Return the task currently protected from cross-hart test dispatch on
/// `hart_id` (0 if no controlled dispatch is reserved).
#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
#[inline(always)]
pub fn test_dispatch_reservation_task_id_for(hart_id: usize) -> usize {
    if hart_id < MAX_HARTS {
        TEST_DISPATCH_RESERVATIONS[hart_id].load(Ordering::Acquire)
    } else {
        0
    }
}

/// Return whether the controlled handoff worker remains queued on `hart_id`.
#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
pub fn test_ready_contains_on_hart(hart_id: usize, task_id: usize) -> bool {
    hart_id < MAX_HARTS
        && HART_LOCALS[hart_id]
            .ready
            .lock()
            .values()
            .any(|queue| queue.iter().any(|&id| id == task_id))
}

/// Whether test-only dispatch setup reserves `task_id` for another hart.
#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
#[inline(always)]
fn test_dispatch_reserved_by_another_hart(thief: usize, task_id: usize) -> bool {
    task_id != 0
        && (0..MAX_HARTS).any(|reserved_hart| {
            reserved_hart != thief
                && TEST_DISPATCH_RESERVATIONS[reserved_hart].load(Ordering::Acquire) == task_id
        })
}

#[cfg(not(all(feature = "test-hooks", target_arch = "riscv64")))]
#[inline(always)]
fn test_dispatch_reserved_by_another_hart(_thief: usize, _task_id: usize) -> bool {
    false
}

const RT_PRIO: u8 = api::TaskPriority::RealTime as u8;

/// Capacity claims against one `(hart, priority)` bucket.
///
/// Every field is read and written only while that hart's `ready` leaf lock is
/// held, so Relaxed atomics merely provide static storage. Invariant per
/// non-empty claim set: `queue.capacity() >= queue.len() + publication +
/// rollback`. Ordinary enqueue, steal destination and selection rollback all
/// honor it; none can consume a claim or move it to another hart.
struct BucketClaims {
    /// Slots promised to prepared, unpublished children.
    publication: AtomicU32,
    /// Slots vacated by a selection that may still roll back.
    rollback: AtomicU32,
    /// The bucket key exists only because a publication claim created it.
    created: AtomicBool,
    /// While `rollback != 0`: how many queued entries still precede the slot the
    /// pending selection vacated. Every removal from the bucket that takes an
    /// entry ahead of that slot (steal, `remove_from_all`) lowers it, so the
    /// rollback reinserts at the exact FIFO position, not a stale index.
    rollback_ahead: core::sync::atomic::AtomicUsize,
}
#[allow(clippy::declare_interior_mutable_const)]
const NO_CLAIMS: BucketClaims = BucketClaims {
    publication: AtomicU32::new(0),
    rollback: AtomicU32::new(0),
    created: AtomicBool::new(false),
    rollback_ahead: core::sync::atomic::AtomicUsize::new(0),
};
static CLAIMS: [[BucketClaims; 256]; MAX_HARTS] = [const { [NO_CLAIMS; 256] }; MAX_HARTS];

fn claims(hart: usize, priority: u8) -> &'static BucketClaims {
    &CLAIMS[hart][priority as usize]
}
/// Slots that ordinary traffic must leave free. Caller holds `hart`'s lock.
fn promised(hart: usize, priority: u8) -> usize {
    let claims = claims(hart, priority);
    claims.publication.load(PromiseOrdering::Relaxed) as usize
        + claims.rollback.load(PromiseOrdering::Relaxed) as usize
}
/// Remove `queue[index]` for a selection that may still roll back: the vacated
/// slot becomes a rollback promise and the number of entries ahead of it is
/// recorded. Caller holds the hart's ready lock.
#[cfg_attr(not(any(cellos_g1_cpu, feature = "test-hooks")), allow(dead_code))]
fn vacate(queue: &mut VecDeque<usize>, claims: &BucketClaims, index: usize) -> Option<usize> {
    debug_assert!(claims.rollback.load(PromiseOrdering::Relaxed) == 0,
        "one pending selection per hart bucket");
    claims.rollback.store(claims.rollback.load(PromiseOrdering::Relaxed) + 1, PromiseOrdering::Relaxed);
    claims.rollback_ahead.store(index, PromiseOrdering::Relaxed);
    queue.remove(index)
}
/// Put a vacated selection back at its exact FIFO position: after the entries
/// that preceded it and are still queued, before the ones that followed it.
#[cfg_attr(not(any(cellos_g1_cpu, feature = "test-hooks")), allow(dead_code))]
fn reinsert(queue: &mut VecDeque<usize>, claims: &BucketClaims, tid: usize) {
    let rollback = claims.rollback.load(PromiseOrdering::Relaxed);
    assert!(
        rollback != 0 && queue.len() < queue.capacity(),
        "selection rollback promise lost its reserved slot"
    );
    claims.rollback.store(rollback - 1, PromiseOrdering::Relaxed);
    let ahead = claims.rollback_ahead.load(PromiseOrdering::Relaxed);
    debug_assert!(ahead <= queue.len());
    queue.insert(ahead.min(queue.len()), tid);
}
/// Account for the removal of `queue[index]` against a pending rollback.
fn note_removed(claims: &BucketClaims, index: usize) {
    if claims.rollback.load(PromiseOrdering::Relaxed) != 0 {
        let ahead = claims.rollback_ahead.load(PromiseOrdering::Relaxed);
        if index < ahead {
            claims.rollback_ahead.store(ahead - 1, PromiseOrdering::Relaxed);
        }
    }
}
/// `queue.remove(index)` that keeps a pending rollback cursor exact.
fn remove_tracked(queue: &mut VecDeque<usize>, claims: &BucketClaims, index: usize) -> Option<usize> {
    note_removed(claims, index);
    queue.remove(index)
}
/// Drop every queued copy of `id`, keeping a pending rollback cursor exact.
fn forget_tracked(queue: &mut VecDeque<usize>, claims: &BucketClaims, id: usize) {
    let pending = claims.rollback.load(PromiseOrdering::Relaxed) != 0;
    let ahead = claims.rollback_ahead.load(PromiseOrdering::Relaxed);
    let (mut position, mut removed_ahead) = (0usize, 0usize);
    queue.retain(|&queued| {
        let keep = queued != id;
        if !keep && position < ahead { removed_ahead += 1; }
        position += 1;
        keep
    });
    if pending {
        claims.rollback_ahead.store(ahead - removed_ahead, PromiseOrdering::Relaxed);
    }
}
/// Remove a key that exists only to hold a now-released publication claim.
fn drop_unused_bucket(ready: &mut BTreeMap<u8, VecDeque<usize>>, hart: usize, priority: u8) {
    let claims = claims(hart, priority);
    if claims.publication.load(PromiseOrdering::Relaxed) == 0
        && claims.rollback.load(PromiseOrdering::Relaxed) == 0
        && claims.created.load(PromiseOrdering::Relaxed)
        && ready.get(&priority).is_some_and(|queue| queue.is_empty())
    {
        ready.remove(&priority);
        claims.created.store(false, PromiseOrdering::Relaxed);
    }
}

/// Hart selected for a newly runnable task of `priority` (RealTime goes to the
/// dedicated hart when online, otherwise the calling hart).
pub(crate) fn destination_hart(priority: u8) -> usize {
    if priority >= RT_PRIO && crate::task::smp::is_rt_hart_online() {
        crate::task::smp::HART_RT
    } else {
        super::current_hart_id()
    }
}

/// Canonical queue slot promised to a prepared child, pinned to the hart that
/// was selected when it was reserved.
///
/// `commit` appends the task without allocating or failing; `Drop` releases the
/// promise. Other pushes, steals and selection rollbacks keep the slot free,
/// and it never migrates to another hart even if an RT hart comes online
/// between reserve and commit.
pub(crate) struct PublicationReservation {
    hart: usize,
    priority: u8,
    armed: bool,
}
impl PublicationReservation {
    /// Bytes of queue storage one promised slot represents.
    pub(crate) const SLOT_BYTES: usize = core::mem::size_of::<usize>();

    pub(crate) fn reserve(priority: u8) -> Result<Self, types::ViError> {
        let hart = destination_hart(priority);
        if hart >= MAX_HARTS {
            return Err(types::ViError::InvalidInput);
        }
        let mut ready = HART_LOCALS[hart].ready.lock();
        #[cfg(cellos_p01_trace)]
        let _semantic = crate::memory::heap::p01_owner::creation(None,
            crate::memory::heap::p01_owner::SCHEDULER_SHARED);
        let claims = claims(hart, priority);
        let publication = claims.publication.load(PromiseOrdering::Relaxed);
        let wanted = promised(hart, priority) + 1;
        let existed = ready.contains_key(&priority);
        let refused = {
            let queue = ready.entry(priority).or_default();
            queue.capacity() < queue.len() + wanted && queue.try_reserve(wanted).is_err()
        };
        if refused || publication == u32::MAX {
            drop_unused_bucket_if_new(&mut ready, hart, priority, existed);
            return Err(types::ViError::OutOfMemory);
        }
        if !existed {
            claims.created.store(true, PromiseOrdering::Relaxed);
        }
        claims.publication.store(publication + 1, PromiseOrdering::Relaxed);
        Ok(Self { hart, priority, armed: true })
    }

    /// The hart and priority the promise is pinned to; the integrating
    /// scheduler sends its preemption notification to exactly this hart.
    pub(crate) fn destination(&self) -> (usize, u8) {
        (self.hart, self.priority)
    }

    pub(crate) fn commit(mut self, tid: usize) {
        {
            let mut ready = HART_LOCALS[self.hart].ready.lock();
            let claims = claims(self.hart, self.priority);
            let publication = claims.publication.load(PromiseOrdering::Relaxed);
            let queue = ready.get_mut(&self.priority).expect("promised ready bucket");
            assert!(
                publication != 0 && queue.len() < queue.capacity(),
                "ready publication promise lost its reserved slot"
            );
            claims.publication.store(publication - 1, PromiseOrdering::Relaxed);
            claims.created.store(false, PromiseOrdering::Relaxed);
            queue.push_back(tid);
        }
        self.armed = false;
    }
}
impl Drop for PublicationReservation {
    fn drop(&mut self) {
        if self.armed {
            let mut ready = HART_LOCALS[self.hart].ready.lock();
            let claims = claims(self.hart, self.priority);
            let publication = claims.publication.load(PromiseOrdering::Relaxed);
            claims.publication.store(publication.saturating_sub(1), PromiseOrdering::Relaxed);
            drop_unused_bucket(&mut ready, self.hart, self.priority);
        }
    }
}
/// Undo a bucket key created by a refused reservation or steal destination.
fn drop_unused_bucket_if_new(
    ready: &mut BTreeMap<u8, VecDeque<usize>>,
    hart: usize,
    priority: u8,
    existed: bool,
) {
    if !existed
        && promised(hart, priority) == 0
        && ready.get(&priority).is_some_and(|queue| queue.is_empty())
    {
        ready.remove(&priority);
    }
}

/// (queued, capacity, publication promises, rollback promises) of one bucket;
/// regression tests use it to prove capacity always covers every promise.
#[cfg(feature = "test-hooks")]
pub(crate) fn test_shape(hart: usize, priority: u8) -> (usize, usize, usize, usize) {
    let ready = HART_LOCALS[hart].ready.lock();
    let claims = claims(hart, priority);
    let (len, capacity) = ready.get(&priority).map_or((0, 0), |q| (q.len(), q.capacity()));
    (len, capacity,
        claims.publication.load(PromiseOrdering::Relaxed) as usize,
        claims.rollback.load(PromiseOrdering::Relaxed) as usize)
}
/// Whether this target invokes `vi_context_switch_complete` from the incoming
/// stack of every raw context switch.
///
/// RV64 and x86_64 have this assembly hook. Other targets retain their
/// established immediate Ready publication.
pub const HAS_INCOMING_SWITCH_COMPLETION_HOOK: bool =
    cfg!(any(target_arch = "riscv64", target_arch = "x86_64"));

#[cfg(any(target_arch = "riscv64", target_arch = "x86_64"))]
const _: [(); 1] = [(); HAS_INCOMING_SWITCH_COMPLETION_HOOK as usize];
#[cfg(any(target_arch = "riscv32", target_arch = "aarch64"))]
const _: [(); 0] = [(); HAS_INCOMING_SWITCH_COMPLETION_HOOK as usize];
/// Whether task→boot must clear scheduler identity before the raw switch.
///
/// Hook-backed targets defer this publication until the outgoing save has
/// completed on the incoming stack. Other targets retain their existing path.
pub const CLEAR_TASK_TO_BOOT_IDENTITY_BEFORE_SWITCH: bool = !HAS_INCOMING_SWITCH_COMPLETION_HOOK;

#[cfg(any(target_arch = "riscv64", target_arch = "x86_64"))]
const _: [(); 0] = [(); CLEAR_TASK_TO_BOOT_IDENTITY_BEFORE_SWITCH as usize];
#[cfg(any(target_arch = "riscv32", target_arch = "aarch64"))]
const _: [(); 1] = [(); CLEAR_TASK_TO_BOOT_IDENTITY_BEFORE_SWITCH as usize];

/// Whether this target can prove the outgoing Context save from the incoming
/// side of the same raw switch.  Keep Ready publication architecture-neutral:
/// targets without that boundary retain their established queue semantics
/// rather than acquiring a permanent RV64-only deferred-requeue state.
pub const HAS_OUTGOING_CONTEXT_SAVE_HOOK: bool =
    cfg!(any(target_arch = "riscv64", target_arch = "x86_64"));

#[cfg(any(target_arch = "riscv64", target_arch = "x86_64"))]
const _: [(); 1] = [(); HAS_OUTGOING_CONTEXT_SAVE_HOOK as usize];
#[cfg(any(target_arch = "riscv32", target_arch = "aarch64"))]
const _: [(); 0] = [(); HAS_OUTGOING_CONTEXT_SAVE_HOOK as usize];

/// Push task `id` with `priority` onto `hart_id`'s local ready queue.
///
/// Call while holding SCHEDULER (lock order: SCHEDULER → ready).
pub fn push_on_hart(hart_id: usize, id: usize, priority: u8) {
    if hart_id < MAX_HARTS {
        let mut ready = HART_LOCALS[hart_id].ready.lock();
        #[cfg(cellos_p01_trace)]
        let _semantic = crate::memory::heap::p01_owner::creation(None,
            crate::memory::heap::p01_owner::SCHEDULER_SHARED);
        // A wake/requeue is infallible and never consumes a promise: keep the
        // outstanding publication/rollback slots free beside the new entry.
        // Growth here is the pre-existing infallible ready-queue growth and
        // happens before the entry is linked, so it cannot hide a dropped wake.
        let keep = promised(hart_id, priority) + 1;
        let queue = ready.entry(priority).or_default();
        if queue.capacity() < queue.len() + keep {
            queue.reserve(keep);
        }
        queue.push_back(id);
    }
}

/// Push task `id` with `priority` onto the CALLING hart's local queue.
pub fn push_on_current_hart(id: usize, priority: u8) {
    push_on_hart(super::current_hart_id(), id, priority);
}

/// Return whether `task_id` still owns a Context on a different hart.
///
/// A task that has become externally wakeable may already have been placed on
/// a ready queue, but its origin hart can still be executing its old stack
/// until the incoming side of that hart's raw switch acknowledges the save.
/// Selection and work stealing must leave that queue entry deferred throughout
/// every published ownership window.
#[inline(always)]
pub fn owned_by_another_hart(hart_id: usize, task_id: usize) -> bool {
    task_id != 0
        && (0..MAX_HARTS).any(|owner_hart| {
            owner_hart != hart_id
                && (current_task_id_for(owner_hart) == task_id
                    || selected_task_id_for(owner_hart) == task_id
                    || executing_task_id_for(owner_hart) == task_id
                    || outgoing_context_save_task_id_for(owner_hart) == task_id)
        })
}

#[cfg(cellos_g1_cpu)]
pub(crate) struct SelectionReservation {
    hart: usize, priority: u8, tid: usize, armed: bool,
}
#[cfg(cellos_g1_cpu)]
impl Drop for SelectionReservation {
    /// Successful selection (or abandoned rollback) releases the vacated slot.
    fn drop(&mut self) {
        if self.armed {
            let _ready = HART_LOCALS[self.hart].ready.lock();
            let claims = claims(self.hart, self.priority);
            let rollback = claims.rollback.load(PromiseOrdering::Relaxed);
            claims.rollback.store(rollback.saturating_sub(1), PromiseOrdering::Relaxed);
        }
    }
}

#[cfg(cellos_g1_cpu)]
pub(crate) fn restore_selection(mut reservation: SelectionReservation) {
    // Stealing does not hold SCHEDULER, so the vacated slot is protected by
    // the rollback promise taken at removal, not by lock ownership. Steals that
    // took entries ahead of it adjusted the recorded position meanwhile.
    let mut ready = HART_LOCALS[reservation.hart].ready.lock();
    let claims = claims(reservation.hart, reservation.priority);
    let queue = ready.get_mut(&reservation.priority).expect("reserved ready queue");
    reinsert(queue, claims, reservation.tid);
    reservation.armed = false;
}
/// Pop the highest-priority task whose Context is not owned by another hart.
///
/// Ineligible entries stay in place rather than being popped and requeued:
/// that preserves priority/FIFO order and prevents a protected-only queue from
/// spinning the scheduler or losing its wake.
pub fn pick_local_eligible(hart_id: usize) -> Option<usize> {
    pick_local_eligible_with(hart_id,
        #[cfg(cellos_g1_cpu)] &|_| true,
        #[cfg(cellos_g1_cpu)] &mut None,
    )
}

pub(crate) fn pick_local_eligible_with(hart_id: usize,
    #[cfg(cellos_g1_cpu)] eligible: &impl Fn(usize) -> bool,
    #[cfg(cellos_g1_cpu)] reservation: &mut Option<SelectionReservation>,
) -> Option<usize> {
    if hart_id >= MAX_HARTS {
        return None;
    }
    let mut rq = HART_LOCALS[hart_id].ready.lock();
    for (&_priority, queue) in rq.iter_mut().rev() {
        if let Some(index) = queue
            .iter()
            .position(|&id| !owned_by_another_hart(hart_id, id)
                && {
                    #[cfg(cellos_g1_cpu)] { eligible(id) }
                    #[cfg(not(cellos_g1_cpu))] { true }
                })
        {
            #[cfg(cellos_g1_cpu)]
            {
                // The removed slot stays promised to this selection until it is
                // consumed (success drops the reservation) or reinserted.
                let tid = queue[index];
                let removed = vacate(queue, claims(hart_id, _priority), index);
                *reservation = Some(SelectionReservation {
                    hart: hart_id, priority: _priority, tid, armed: true });
                return removed;
            }
            #[cfg(not(cellos_g1_cpu))]
            return queue.remove(index);
        }
    }
    None
}

/// Remove task `id` from every hart's ready queue.
/// Call while holding SCHEDULER (lock order: SCHEDULER → ready).
pub fn remove_from_all(id: usize) {
    for (hart, local) in HART_LOCALS.iter().enumerate() {
        let mut rq = local.ready.lock();
        for (&priority, queue) in rq.iter_mut() {
            forget_tracked(queue, claims(hart, priority), id);
        }
    }
}

/// Total ready-task count summed across all harts.
pub fn total_ready_count() -> usize {
    (0..MAX_HARTS)
        .map(|h| {
            HART_LOCALS[h]
                .ready
                .lock()
                .values()
                .map(|q| q.len())
                .sum::<usize>()
        })
        .sum()
}

/// Task selected for `hart_id`. This changes before a raw context switch.
#[inline(always)]
pub fn current_task_id_for(hart_id: usize) -> usize {
    if hart_id < MAX_HARTS {
        HART_LOCALS[hart_id]
            .current_task_id
            .load(core::sync::atomic::Ordering::Acquire)
    } else {
        0
    }
}

/// Publish the task selected for `hart_id` (0 = idle).
#[inline(always)]
pub fn set_current_task_id(hart_id: usize, id: usize) {
    if hart_id < MAX_HARTS {
        HART_LOCALS[hart_id]
            .current_task_id
            .store(id, core::sync::atomic::Ordering::Release);
    }
}
/// Incoming task pinned for an in-flight raw context switch.
///
/// Hook-backed targets retain this pin until the incoming raw-switch callback.
#[inline(always)]
pub fn selected_task_id_for(hart_id: usize) -> usize {
    if hart_id < MAX_HARTS {
        HART_LOCALS[hart_id]
            .selected_task_id
            .load(core::sync::atomic::Ordering::Acquire)
    } else {
        0
    }
}

/// Pin `id` from scheduler selection through incoming-side switch completion.
#[inline(always)]
pub fn set_selected_task_id(hart_id: usize, id: usize) {
    if hart_id < MAX_HARTS {
        HART_LOCALS[hart_id]
            .selected_task_id
            .store(id, core::sync::atomic::Ordering::Release);
    }
}

/// Publish the incoming execution identity before releasing its selection pin.
///
/// Readers load `selected_task_id` first and `executing_task_id` second. If
/// they observe the cleared selection with Acquire ordering, this Release store
/// guarantees that the preceding executing publication is also visible.
#[inline(always)]
pub fn complete_selected_switch(hart_id: usize, id: usize) {
    if hart_id < MAX_HARTS {
        HART_LOCALS[hart_id]
            .executing_task_id
            .store(id, core::sync::atomic::Ordering::Release);
        HART_LOCALS[hart_id]
            .selected_task_id
            .store(0, core::sync::atomic::Ordering::Release);
    }
}

/// Release a selection that did not reach the raw context switch.
#[inline(always)]
pub fn abort_selected_switch(hart_id: usize) {
    if hart_id < MAX_HARTS {
        HART_LOCALS[hart_id]
            .selected_task_id
            .store(0, core::sync::atomic::Ordering::Release);
    }
}

/// Task whose saved context has actually become active on `hart_id`.
#[inline(always)]
pub fn executing_task_id_for(hart_id: usize) -> usize {
    if hart_id >= MAX_HARTS {
        return 0;
    }
    #[cfg(any(target_arch = "riscv64", target_arch = "x86_64"))]
    {
        HART_LOCALS[hart_id]
            .executing_task_id
            .load(core::sync::atomic::Ordering::Acquire)
    }
    #[cfg(not(any(target_arch = "riscv64", target_arch = "x86_64")))]
    {
        current_task_id_for(hart_id)
    }
}

/// Called only from the incoming side of a completed raw context switch.
#[inline(always)]
pub fn set_executing_task_id(hart_id: usize, id: usize) {
    if hart_id < MAX_HARTS {
        HART_LOCALS[hart_id]
            .executing_task_id
            .store(id, core::sync::atomic::Ordering::Release);
    }
}

/// Publish that `task_id` is Ready but its outgoing Context has not yet been
/// saved by the raw switch. Hook-backed targets release this from the incoming
/// callback; other targets keep their established queue semantics.
#[inline(always)]
pub fn begin_outgoing_context_save(hart_id: usize, task_id: usize) {
    #[cfg(any(target_arch = "riscv64", target_arch = "x86_64"))]
    if hart_id < MAX_HARTS {
        HART_LOCALS[hart_id]
            .outgoing_context_save_task_id
            .store(task_id, core::sync::atomic::Ordering::Release);
    }
    #[cfg(not(any(target_arch = "riscv64", target_arch = "x86_64")))]
    {
        let _ = (hart_id, task_id);
    }
}

/// Clear the outgoing Context-save guard only after the raw switch has saved
/// the old Context and execution is on the incoming stack.
#[inline(always)]
pub fn complete_outgoing_context_save(hart_id: usize) {
    #[cfg(any(target_arch = "riscv64", target_arch = "x86_64"))]
    if hart_id < MAX_HARTS {
        HART_LOCALS[hart_id]
            .outgoing_context_save_task_id
            .store(0, core::sync::atomic::Ordering::Release);
    }
    #[cfg(not(any(target_arch = "riscv64", target_arch = "x86_64")))]
    {
        let _ = hart_id;
    }
}

/// Return the locally requeued task whose Context save has not completed.
#[inline(always)]
pub fn outgoing_context_save_task_id_for(hart_id: usize) -> usize {
    #[cfg(any(target_arch = "riscv64", target_arch = "x86_64"))]
    {
        if hart_id < MAX_HARTS {
            return HART_LOCALS[hart_id]
                .outgoing_context_save_task_id
                .load(core::sync::atomic::Ordering::Acquire);
        }
    }
    #[cfg(not(any(target_arch = "riscv64", target_arch = "x86_64")))]
    {
        let _ = hart_id;
    }
    0
}

/// Returns true while `task_id` is selected or executing on any hart.
///
/// The selected load must precede the executing load. Completion publishes the
/// executing identity before clearing selection, so an observer cannot accept a
/// false quiescent gap between those two ownership states.
pub fn any_hart_running(task_id: usize) -> bool {
    (0..MAX_HARTS).any(|hart| {
        selected_task_id_for(hart) == task_id
            || current_task_id_for(hart) == task_id
            || executing_task_id_for(hart) == task_id
            || outgoing_context_save_task_id_for(hart) == task_id
    })
}

/// Move up to `ceil(stealable/2)` Normal/Background tasks from the busiest other
/// hart's queue into `thief`'s queue.  Never steals RT tasks.
///
/// Always locks hart 0 then hart 1 (ABBA-safe for MAX_HARTS=2).
pub fn steal_from_busiest(thief: usize) {
    steal_from_busiest_with(thief,
        #[cfg(cellos_g1_cpu)] &|_| true,
    );
}

pub(crate) fn steal_from_busiest_with(thief: usize,
    #[cfg(cellos_g1_cpu)] eligible: &impl Fn(usize) -> bool,
) {
    if thief >= MAX_HARTS {
        return;
    }
    // Only 2 harts: victim is always the other one.
    let victim = 1 - thief;

    // Acquire in hart-id order to prevent ABBA deadlock.
    let mut g0 = HART_LOCALS[0].ready.lock();
    let mut g1 = HART_LOCALS[1].ready.lock();
    #[cfg(cellos_p01_trace)]
    let _semantic = crate::memory::heap::p01_owner::creation(None,
        crate::memory::heap::p01_owner::SCHEDULER_SHARED);
    let (victim_map, thief_map) = if victim == 0 {
        (&mut *g0, &mut *g1)
    } else {
        (&mut *g1, &mut *g0)
    };

    // Count only tasks whose Context is not owned by another hart and which
    // are not reserved by test-only controlled dispatch setup. The ownership
    // snapshot includes current, selected, executing, and outgoing save
    // windows; a stale ownership value merely defers work until the next pick
    // and can never permit simultaneous execution.
    let permits = |id: usize| -> bool {
        !owned_by_another_hart(thief, id)
            && !test_dispatch_reserved_by_another_hart(thief, id)
            && {
                #[cfg(cellos_g1_cpu)] { eligible(id) }
                #[cfg(not(cellos_g1_cpu))] { true }
            }
    };
    let stealable: usize = victim_map
        .iter()
        .filter(|(&p, _)| p < RT_PRIO)
        .map(|(_, q)| q.iter().filter(|&&id| permits(id)).count())
        .sum();
    if stealable == 0 {
        return;
    }
    let to_steal = (stealable / 2).max(1);

    // Move tasks, highest-priority first (Normal before Background). The
    // destination bucket must have headroom for the entry *and* every outstanding
    // promise before the victim entry is removed; on refusal the victim stays
    // queued and stealing stops.
    let mut stolen = 0;
    'priorities: for p in (0..RT_PRIO).rev() {
        while stolen < to_steal {
            let Some(index) = victim_map
                .get(&p)
                .and_then(|queue| queue.iter().position(|&id| permits(id)))
            else {
                break;
            };
            let existed = thief_map.contains_key(&p);
            let keep = promised(thief, p) + 1;
            let refused = {
                let dest = thief_map.entry(p).or_default();
                dest.capacity() < dest.len() + keep && dest.try_reserve(keep).is_err()
            };
            if refused {
                drop_unused_bucket_if_new(thief_map, thief, p, existed);
                break 'priorities;
            }
            let id = victim_map
                .get_mut(&p)
                .and_then(|queue| remove_tracked(queue, claims(victim, p), index))
                .expect("position returned an in-bounds ready task");
            thief_map
                .get_mut(&p)
                .expect("destination bucket reserved before removal")
                .push_back(id);
            stolen += 1;
        }
        if stolen >= to_steal {
            break;
        }
    }
}

/// Regression probe for selection rollback ordering, run by the publication
/// tests on every hook build (the `cellos_g1_cpu` caller uses the same
/// `vacate`/`reinsert`/`remove_tracked`/`forget_tracked` primitives): a vacated
/// selection returns to its exact FIFO position even after entries ahead of it
/// were stolen or forgotten. `base` is a tid range the caller owns.
#[cfg(feature = "test-hooks")]
#[allow(clippy::declare_interior_mutable_const)]
pub(crate) fn test_rollback_keeps_fifo_position(base: usize) -> bool {
    // Pure queue logic: steals ahead of and behind the vacated slot.
    let local = NO_CLAIMS;
    let mut queue: VecDeque<usize> = (1..=6).collect();
    queue.reserve(2);
    let tid = queue[2];
    if vacate(&mut queue, &local, 2) != Some(tid) { return false; }
    remove_tracked(&mut queue, &local, 0);
    remove_tracked(&mut queue, &local, 2);
    reinsert(&mut queue, &local, tid);
    if queue != [2, 3, 4, 6] || local.rollback.load(PromiseOrdering::Relaxed) != 0 { return false; }
    let local = NO_CLAIMS;
    let mut queue: VecDeque<usize> = (1..=6).collect();
    queue.reserve(2);
    let tid = queue[3];
    if vacate(&mut queue, &local, 3) != Some(tid) { return false; }
    forget_tracked(&mut queue, &local, 2);
    forget_tracked(&mut queue, &local, 6);
    reinsert(&mut queue, &local, tid);
    if queue != [1, 3, 4, 5] { return false; }
    // Real buckets: the production steal runs while the selection is vacated.
    if super::super::smp::is_rt_hart_online() || total_ready_count() != 0 { return true; }
    let hart = super::current_hart_id();
    if hart >= MAX_HARTS || MAX_HARTS < 2 { return true; }
    let victim = 1 - hart;
    let priority = api::TaskPriority::Normal as u8;
    let tids: [usize; 6] = core::array::from_fn(|n| base + n);
    for tid in tids { push_on_hart(victim, tid, priority); }
    let selected = {
        let mut ready = HART_LOCALS[victim].ready.lock();
        let Some(queue) = ready.get_mut(&priority) else { return false };
        vacate(queue, claims(victim, priority), 2)
    };
    steal_from_busiest(hart);
    let order: alloc::vec::Vec<usize> = {
        let mut ready = HART_LOCALS[victim].ready.lock();
        let Some(queue) = ready.get_mut(&priority) else { return false };
        reinsert(queue, claims(victim, priority), tids[2]);
        queue.iter().copied().collect()
    };
    let stolen: alloc::vec::Vec<usize> = HART_LOCALS[hart].ready.lock().get(&priority)
        .map(|queue| queue.iter().copied().collect()).unwrap_or_default();
    for tid in tids { remove_from_all(tid); }
    selected == Some(tids[2])
        && stolen == [tids[0], tids[1]]
        && order == [tids[2], tids[3], tids[4], tids[5]]
        && test_shape(victim, priority).3 == 0
        && total_ready_count() == 0
}
