// SPDX-License-Identifier: MPL-2.0
//! `hart_local`: `crate::sched::hart_local`'s per-hart addressing on aarch64.
//! A CPU's `MPIDR_EL1` affinity maps to a CPU index (`smp::CPU_MPIDRS`) and
//! from there to its logical hart (`smp::CPU_TOPOLOGY`), whose slot of
//! `HART_LOCALS` it owns.

use crate::sched::hart_local::{ViHartLocal, HART_LOCALS};
use crate::sched::smp::{CPU_MPIDRS, CPU_TOPOLOGY};
use core::sync::atomic::Ordering;

/// Exclusive bound on the physical ids `smp::CPU_TOPOLOGY` can map. AArch64
/// keeps the CPU-index table it had, sized `MAX_HARTS`, so a CPU index at or
/// above `MAX_HARTS` stays unmapped (it resolves to slot 0).
pub const PHYSICAL_ID_LIMIT: usize = crate::sched::smp::MAX_HARTS;

/// The CPU index this hart is running on: its `MPIDR_EL1` affinity looked up
/// in [`CPU_MPIDRS`]. `None` for a core the firmware tree does not describe.
#[inline(always)]
pub fn physical_cpu_index() -> Option<usize> {
    CPU_MPIDRS.cpu_index(crate::arch::smp::mpidr())
}

/// Publish `cpu_index -> logical_hart` before `cpu_index` starts executing.
///
/// `false` when the pair conflicts with one already published (or an index is
/// out of range); the map is then unchanged. Re-publishing the same pair, as a
/// started hart's `install()` does, is accepted.
pub fn publish_physical_cpu(cpu_index: usize, logical_hart: usize) -> bool {
    CPU_TOPOLOGY.publish(logical_hart, cpu_index)
}

/// The CPU index a logical hart runs on, if it was published.
pub fn physical_cpu_for(logical_hart: usize) -> Option<usize> {
    CPU_TOPOLOGY.physical(logical_hart)
}

/// Slot owned by the calling CPU.
#[inline(always)]
fn owned_slot_index() -> usize {
    physical_cpu_index()
        .and_then(|cpu| CPU_TOPOLOGY.logical(cpu))
        .unwrap_or(0)
}

/// [`crate::sched::hart_local::current_hart`]: the calling CPU's owned slot.
#[inline(always)]
pub(crate) unsafe fn current_hart() -> &'static ViHartLocal {
    &HART_LOCALS[owned_slot_index()]
}

/// [`crate::sched::hart_local::installed_hart_id`]: a core the topology does
/// not describe has no slot.
#[inline(always)]
pub(crate) fn installed_hart_id() -> Option<usize> {
    physical_cpu_index().and_then(|cpu| CPU_TOPOLOGY.logical(cpu))
}

/// [`crate::sched::hart_local::current_hart_id`].
pub(crate) fn current_hart_id() -> usize {
    owned_slot_index()
}

/// [`crate::sched::hart_local::current_cell_id`].
#[inline(always)]
pub(crate) fn current_cell_id() -> usize {
    HART_LOCALS[owned_slot_index()]
        .current_cell_id
        .load(Ordering::Relaxed)
}

/// [`crate::sched::hart_local::set_current_cell_id`].
#[inline(always)]
pub(crate) fn set_current_cell_id(id: usize) {
    HART_LOCALS[owned_slot_index()]
        .current_cell_id
        .store(id, Ordering::Relaxed);
}

/// No-op: no `tp`-based hart-local addressing on this target.
///
/// # Safety
/// No preconditions on these targets — `_val` is unused and no state is written.
pub(crate) unsafe fn write_tp(_val: usize) {}

/// The last step of [`crate::sched::hart_local::install`]: publish this CPU's
/// logical hart.
pub(crate) fn install_tail(hart_id: usize) {
    // A refused pair (or a core the firmware tree does not describe) leaves
    // this CPU resolving to slot 0, as an unpublished one always did; the boot
    // path that started it already reported the conflict.
    if let Some(cpu) = physical_cpu_index() {
        let _ = publish_physical_cpu(cpu, hart_id);
    }
}
