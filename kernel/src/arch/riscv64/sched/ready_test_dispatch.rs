// SPDX-License-Identifier: MPL-2.0
//! riscv64 items of `crate::sched::hart_local::ready`, compiled as its `rv` child module
//! (`#[path]`), so they keep reading that module's private items unchanged.

use super::*;
use core::sync::atomic::{AtomicUsize, Ordering};

/// Test-only dispatch reservation. The context-handoff regression uses this
/// to keep its Normal-priority worker on hart 1 until that worker has begun
/// the controlled block-before-yield handoff.
pub(super) static TEST_DISPATCH_RESERVATIONS: [AtomicUsize; MAX_HARTS] =
    [const { AtomicUsize::new(0) }; MAX_HARTS];

/// Reserve a test worker's initial dispatch on `hart_id`.
///
/// The reservation affects only work stealing; local selection on the
/// reserved hart remains normal scheduler behavior.
pub fn reserve_test_dispatch_on_hart(hart_id: usize, task_id: usize) -> bool {
    task_id != 0
        && hart_id < MAX_HARTS
        && TEST_DISPATCH_RESERVATIONS[hart_id]
            .compare_exchange(0, task_id, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
}

/// Release a test worker's initial-dispatch reservation.
pub fn release_test_dispatch_on_hart(hart_id: usize, task_id: usize) -> bool {
    hart_id < MAX_HARTS
        && TEST_DISPATCH_RESERVATIONS[hart_id]
            .compare_exchange(task_id, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
}

/// Return the task currently protected from cross-hart test dispatch on
/// `hart_id` (0 if no controlled dispatch is reserved).
#[inline(always)]
pub fn test_dispatch_reservation_task_id_for(hart_id: usize) -> usize {
    if hart_id < MAX_HARTS {
        TEST_DISPATCH_RESERVATIONS[hart_id].load(Ordering::Acquire)
    } else {
        0
    }
}

/// Return whether the controlled handoff worker remains queued on `hart_id`.
pub fn test_ready_contains_on_hart(hart_id: usize, task_id: usize) -> bool {
    hart_id < MAX_HARTS
        && HART_LOCALS[hart_id]
            .ready
            .lock()
            .values()
            .any(|queue| queue.iter().any(|&id| id == task_id))
}

/// Whether test-only dispatch setup reserves `task_id` for another hart.
#[inline(always)]
pub(super) fn test_dispatch_reserved_by_another_hart(thief: usize, task_id: usize) -> bool {
    task_id != 0
        && (0..MAX_HARTS).any(|reserved_hart| {
            reserved_hart != thief
                && TEST_DISPATCH_RESERVATIONS[reserved_hart].load(Ordering::Acquire) == task_id
        })
}
