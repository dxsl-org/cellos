// SPDX-License-Identifier: MPL-2.0
//! `sched`: the scheduler's context-switch seam.
//!
//! The switch itself is the HAL's: `switch(SwitchPlan { out, inc, root, irq })`.
//! x86_64 never writes CR3 in the switch; `root: None` inherits the incoming
//! context's saved `user_cr3` (see `hal::arch::SwitchPlan`).

pub use crate::hal::arch::{set_kernel_stack_for, switch, IrqSaved, Root, SwitchPlan};

/// `yield_cpu` masks interrupts on entry, before any scheduler lock: a LAPIC
/// tick inside the lock would re-enter the scheduler and deadlock on the same
/// spinlock.
pub const MASK_AT_ENTRY: bool = true;

/// The scheduler logs `[sched] hart N dispatched a task` once per CPU.
pub const REPORTS_DISPATCH: bool = true;

/// The root for a top-level table at physical address `table` with tag `asid`
/// (the PML4 address itself).
#[inline(always)]
pub fn root_for_table(table: usize, asid: usize) -> Option<Root> {
    Root::new(table, asid)
}
