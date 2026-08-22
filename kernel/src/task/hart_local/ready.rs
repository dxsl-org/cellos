//! Per-hart ready-queue helpers for the Phase 03 work-stealing scheduler.
//!
//! Lock order: SCHEDULER (global, coarse) → per-hart ready lock (leaf).
//! `steal_from_busiest` holds only leaf locks — never SCHEDULER.
//!
//! RT tasks (priority ≥ RealTime) are never stolen; Phase 04 will hart-pin them.

use super::{HART_LOCALS, MAX_HARTS};

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
/// Whether this target invokes `vi_context_switch_complete` from the incoming
/// stack of every raw context switch.
///
/// RV64 has that assembly hook. RV32, AArch64, and x86_64 must publish an
/// outgoing Ready task before switching so it cannot be stranded waiting for a
/// callback those architectures do not provide.
pub const HAS_INCOMING_SWITCH_COMPLETION_HOOK: bool = cfg!(target_arch = "riscv64");

#[cfg(target_arch = "riscv64")]
const _: [(); 1] = [(); HAS_INCOMING_SWITCH_COMPLETION_HOOK as usize];
#[cfg(any(target_arch = "riscv32", target_arch = "aarch64", target_arch = "x86_64"))]
const _: [(); 0] = [(); HAS_INCOMING_SWITCH_COMPLETION_HOOK as usize];
/// Whether task→boot must clear scheduler identity before the raw switch.
///
/// RV64 defers this publication to `vi_context_switch_complete`, after the
/// outgoing Context has been saved. The other targets have no such callback,
/// so retaining the old task through a boot switch would strand a subsequently
/// requeued task behind stale ownership.
pub const CLEAR_TASK_TO_BOOT_IDENTITY_BEFORE_SWITCH: bool =
    !HAS_INCOMING_SWITCH_COMPLETION_HOOK;

#[cfg(target_arch = "riscv64")]
const _: [(); 0] = [(); CLEAR_TASK_TO_BOOT_IDENTITY_BEFORE_SWITCH as usize];
#[cfg(any(target_arch = "riscv32", target_arch = "aarch64", target_arch = "x86_64"))]
const _: [(); 1] = [(); CLEAR_TASK_TO_BOOT_IDENTITY_BEFORE_SWITCH as usize];


/// Whether this target can prove the outgoing Context save from the incoming
/// side of the same raw switch.  Keep Ready publication architecture-neutral:
/// targets without that boundary retain their established queue semantics
/// rather than acquiring a permanent RV64-only deferred-requeue state.
pub const HAS_OUTGOING_CONTEXT_SAVE_HOOK: bool = cfg!(target_arch = "riscv64");

#[cfg(target_arch = "riscv64")]
const _: [(); 1] = [(); HAS_OUTGOING_CONTEXT_SAVE_HOOK as usize];
#[cfg(any(target_arch = "riscv32", target_arch = "aarch64", target_arch = "x86_64"))]
const _: [(); 0] = [(); HAS_OUTGOING_CONTEXT_SAVE_HOOK as usize];

/// Push task `id` with `priority` onto `hart_id`'s local ready queue.
///
/// Call while holding SCHEDULER (lock order: SCHEDULER → ready).
pub fn push_on_hart(hart_id: usize, id: usize, priority: u8) {
    if hart_id < MAX_HARTS {
        HART_LOCALS[hart_id]
            .ready
            .lock()
            .entry(priority)
            .or_default()
            .push_back(id);
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

/// Pop the highest-priority task whose Context is not owned by another hart.
///
/// Ineligible entries stay in place rather than being popped and requeued:
/// that preserves priority/FIFO order and prevents a protected-only queue from
/// spinning the scheduler or losing its wake.
pub fn pick_local_eligible(hart_id: usize) -> Option<usize> {
    if hart_id >= MAX_HARTS {
        return None;
    }
    let mut rq = HART_LOCALS[hart_id].ready.lock();
    for queue in rq.values_mut().rev() {
        if let Some(index) = queue
            .iter()
            .position(|&id| !owned_by_another_hart(hart_id, id))
        {
            return queue.remove(index);
        }
    }
    None
}

/// Remove task `id` from every hart's ready queue.
/// Call while holding SCHEDULER (lock order: SCHEDULER → ready).
pub fn remove_from_all(id: usize) {
    for local in HART_LOCALS.iter() {
        let mut rq = local.ready.lock();
        for queue in rq.values_mut() {
            queue.retain(|&x| x != id);
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
/// Only RV64 currently has a proven incoming-side completion hook. Other
/// architectures retain their established `current_task_id` publication and
/// do not use this transient slot.
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
    #[cfg(target_arch = "riscv64")]
    {
        HART_LOCALS[hart_id]
            .executing_task_id
            .load(core::sync::atomic::Ordering::Acquire)
    }
    #[cfg(not(target_arch = "riscv64"))]
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
/// saved by the raw switch.  Only RV64 has the matching completion hook; on
/// the other targets this deliberately remains a no-op so their ready queues
/// keep their existing, immediately-stealable semantics.
#[inline(always)]
pub fn begin_outgoing_context_save(hart_id: usize, task_id: usize) {
    #[cfg(target_arch = "riscv64")]
    if hart_id < MAX_HARTS {
        HART_LOCALS[hart_id]
            .outgoing_context_save_task_id
            .store(task_id, core::sync::atomic::Ordering::Release);
    }
    #[cfg(not(target_arch = "riscv64"))]
    {
        let _ = (hart_id, task_id);
    }
}

/// Clear the outgoing Context-save guard only after the raw switch has saved
/// the old Context and execution is on the incoming stack.
#[inline(always)]
pub fn complete_outgoing_context_save(hart_id: usize) {
    #[cfg(target_arch = "riscv64")]
    if hart_id < MAX_HARTS {
        HART_LOCALS[hart_id]
            .outgoing_context_save_task_id
            .store(0, core::sync::atomic::Ordering::Release);
    }
    #[cfg(not(target_arch = "riscv64"))]
    {
        let _ = hart_id;
    }
}

/// Return the locally requeued task whose Context save has not completed.
#[inline(always)]
pub fn outgoing_context_save_task_id_for(hart_id: usize) -> usize {
    #[cfg(target_arch = "riscv64")]
    {
        if hart_id < MAX_HARTS {
            return HART_LOCALS[hart_id]
                .outgoing_context_save_task_id
                .load(core::sync::atomic::Ordering::Acquire);
        }
    }
    #[cfg(not(target_arch = "riscv64"))]
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
    if thief >= MAX_HARTS {
        return;
    }
    // Only 2 harts: victim is always the other one.
    let victim = 1 - thief;

    // Acquire in hart-id order to prevent ABBA deadlock.
    let mut g0 = HART_LOCALS[0].ready.lock();
    let mut g1 = HART_LOCALS[1].ready.lock();

    // Count only tasks whose Context is not owned by another hart and which
    // are not reserved by test-only controlled dispatch setup. The ownership
    // snapshot includes current, selected, executing, and outgoing save
    // windows; a stale ownership value merely defers work until the next pick
    // and can never permit simultaneous execution.
    let stealable: usize = if victim == 0 {
        g0.iter()
            .filter(|(&p, _)| p < RT_PRIO)
            .map(|(_, q)| {
                q.iter()
                    .filter(|&&id| {
                        !owned_by_another_hart(thief, id)
                            && !test_dispatch_reserved_by_another_hart(thief, id)
                    })
                    .count()
            })
            .sum()
    } else {
        g1.iter()
            .filter(|(&p, _)| p < RT_PRIO)
            .map(|(_, q)| {
                q.iter()
                    .filter(|&&id| {
                        !owned_by_another_hart(thief, id)
                            && !test_dispatch_reserved_by_another_hart(thief, id)
                    })
                    .count()
            })
            .sum()
    };
    if stealable == 0 {
        return;
    }
    let to_steal = (stealable / 2).max(1);

    // Move tasks, highest-priority first (Normal before Background).
    let mut stolen = 0;
    for p in (0..RT_PRIO).rev() {
        if stolen >= to_steal {
            break;
        }
        if thief == 0 {
            // victim=1(g1) → thief=0(g0)
            if let Some(vq) = g1.get_mut(&p) {
                while stolen < to_steal {
                    match vq
                        .iter()
                        .position(|&id| {
                            !owned_by_another_hart(thief, id)
                                && !test_dispatch_reserved_by_another_hart(thief, id)
                        })
                    {
                        Some(index) => {
                            let id = vq
                                .remove(index)
                                .expect("position returned an in-bounds ready task");
                            g0.entry(p).or_default().push_back(id);
                            stolen += 1;
                        }
                        None => break,
                    }
                }
            }
        } else {
            // victim=0(g0) → thief=1(g1)
            if let Some(vq) = g0.get_mut(&p) {
                while stolen < to_steal {
                    match vq
                        .iter()
                        .position(|&id| {
                            !owned_by_another_hart(thief, id)
                                && !test_dispatch_reserved_by_another_hart(thief, id)
                        })
                    {
                        Some(index) => {
                            let id = vq
                                .remove(index)
                                .expect("position returned an in-bounds ready task");
                            g1.entry(p).or_default().push_back(id);
                            stolen += 1;
                        }
                        None => break,
                    }
                }
            }
        }
    }
}
