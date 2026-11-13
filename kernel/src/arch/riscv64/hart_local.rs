// SPDX-License-Identifier: MPL-2.0
//! `hart_local`: `crate::sched::hart_local`'s per-hart addressing on riscv64.
//! Each hart keeps `tp` pointing at its own `ViHartLocal`.

use crate::sched::hart_local::ViHartLocal;
use core::sync::atomic::Ordering;

/// Exclusive bound on the physical ids `smp::CPU_TOPOLOGY` can map. RISC-V
/// keeps its boot-hart swap, which only covers hart ids below `MAX_HARTS`.
pub const PHYSICAL_ID_LIMIT: usize = crate::sched::smp::MAX_HARTS;

/// [`crate::sched::hart_local::current_hart`]: read `tp`.
///
/// # Safety
/// `tp` must point to a valid `ViHartLocal` (guaranteed after `install()`).
#[inline(always)]
pub(crate) unsafe fn current_hart() -> &'static ViHartLocal {
    let tp = crate::arch::smp::read_tp();
    &*(tp as *const ViHartLocal)
}

/// [`crate::sched::hart_local::installed_hart_id`]: a secondary runs with
/// `tp = 0` until `install`.
#[inline(always)]
pub(crate) fn installed_hart_id() -> Option<usize> {
    let tp = crate::arch::smp::read_tp();
    (tp != 0).then(crate::sched::hart_local::current_hart_id)
}

/// [`crate::sched::hart_local::current_hart_id`].
pub(crate) fn current_hart_id() -> usize {
    let tp = crate::arch::smp::read_tp();
    if tp == 0 {
        return 0;
    }
    unsafe {
        (*(tp as *const ViHartLocal))
            .hart_id
            .load(Ordering::Relaxed)
    }
}

/// [`crate::sched::hart_local::current_cell_id`].
#[inline(always)]
pub(crate) fn current_cell_id() -> usize {
    let tp = crate::arch::smp::read_tp();
    if tp == 0 {
        return 0;
    }
    unsafe {
        (*(tp as *const ViHartLocal))
            .current_cell_id
            .load(Ordering::Relaxed)
    }
}

/// [`crate::sched::hart_local::set_current_cell_id`].
#[inline(always)]
pub(crate) fn set_current_cell_id(id: usize) {
    let tp = crate::arch::smp::read_tp();
    if tp == 0 {
        return;
    }
    unsafe {
        (*(tp as *const ViHartLocal))
            .current_cell_id
            .store(id, Ordering::Relaxed)
    };
}

/// [`crate::sched::hart_local::write_tp`].
///
/// # Safety
/// As [`crate::sched::hart_local::write_tp`].
#[inline(always)]
pub(crate) unsafe fn write_tp(val: usize) {
    // SAFETY: the caller ensures context (see above).
    crate::arch::smp::write_tp(val);
}

/// The last step of [`crate::sched::hart_local::install`], once `tp` is set:
/// this hart's trap vector.
pub(crate) fn install_tail(hart_id: usize) {
    crate::hal::trap::init_for_hart(hart_id);
}
