// SPDX-License-Identifier: MPL-2.0
//! `sched`: the scheduler's context-switch seam.
//!
//! The switch itself is the HAL's: `switch(SwitchPlan { out, inc, root, irq })`.
//! `root: None` writes no `TTBR0_EL1` (see `hal::arch::SwitchPlan`).

pub use crate::hal::arch::{set_kernel_stack_for, switch, IrqSaved, Root, SwitchPlan};

/// `yield_cpu` masks IRQs at selection, not on entry: the idle loop reaches it
/// with IRQs unmasked, and the housekeeping drains before selection keep the
/// caller's state.
pub const MASK_AT_ENTRY: bool = false;

/// The scheduler logs `[sched] hart N dispatched a task` once per PE.
pub const REPORTS_DISPATCH: bool = true;

/// The root for a top-level table at physical address `table` with tag `asid`
/// (the table address itself; `root_register_value` adds the ASID).
#[inline(always)]
pub fn root_for_table(table: usize, asid: usize) -> Option<Root> {
    Root::new(table, asid)
}
