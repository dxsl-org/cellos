// SPDX-License-Identifier: MPL-2.0
//! `hart_local`: `crate::sched::hart_local`'s per-hart addressing on x86_64:
//! the logical CPU installed in GS names the owned slot of `HART_LOCALS`.

use crate::sched::hart_local::{ViHartLocal, HART_LOCALS};
use core::sync::atomic::Ordering;

/// Exclusive bound on the physical ids `smp::CPU_TOPOLOGY` can map: the xAPIC
/// ID space. A LAPIC ID above 255 needs interrupt remapping for IOAPIC/MSI
/// destinations (plan 261103-2218 phase 12), so such a CPU is refused by
/// bring-up and by `hal::x86_64::apic::register_cpu` alike.
pub const PHYSICAL_ID_LIMIT: usize = 256;

/// Slot owned by the calling CPU.
#[inline(always)]
fn owned_slot_index() -> usize {
    #[cfg(not(all(test, not(target_os = "none"))))]
    {
        hal::syscall::current_cpu_id()
    }
    // Hosted unit fixtures do not install a privileged kernel GS base.
    #[cfg(all(test, not(target_os = "none")))]
    {
        0
    }
}

/// [`crate::sched::hart_local::current_hart`]: the calling CPU's owned slot.
#[inline(always)]
pub(crate) unsafe fn current_hart() -> &'static ViHartLocal {
    &HART_LOCALS[owned_slot_index()]
}

/// [`crate::sched::hart_local::installed_hart_id`].
#[inline(always)]
pub(crate) fn installed_hart_id() -> Option<usize> {
    Some(owned_slot_index())
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

/// The last step of [`crate::sched::hart_local::install`]: nothing on x86_64.
pub(crate) fn install_tail(_hart_id: usize) {}
