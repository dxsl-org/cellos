// SPDX-License-Identifier: MPL-2.0
//! `hart_local` for the 32-bit targets: riscv32 addresses its `ViHartLocal`
//! through `tp`; arm and x86 have one slot (index 0).

/// Exclusive bound on the physical ids `smp::CPU_TOPOLOGY` can map.
pub const PHYSICAL_ID_LIMIT: usize = crate::sched::smp::MAX_HARTS;

/// The last step of [`crate::sched::hart_local::install`]: nothing here.
pub(crate) fn install_tail(_hart_id: usize) {}

#[cfg(not(target_arch = "riscv32"))]
pub(crate) use slot::*;
#[cfg(target_arch = "riscv32")]
pub(crate) use tp::*;

#[cfg(target_arch = "riscv32")]
mod tp {
    use crate::sched::hart_local::ViHartLocal;
    use core::sync::atomic::Ordering;

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
}

#[cfg(not(target_arch = "riscv32"))]
mod slot {
    use crate::sched::hart_local::{ViHartLocal, HART_LOCALS};
    use core::sync::atomic::Ordering;

    /// Slot owned by the calling CPU.
    #[inline(always)]
    fn owned_slot_index() -> usize {
        0
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
}
