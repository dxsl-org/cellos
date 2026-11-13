// SPDX-License-Identifier: MPL-2.0
//! `sched`: the scheduler's context-switch seam.
//!
//! The switch itself is the HAL's: `switch(SwitchPlan { out, inc, root, irq })`.
//! `root: None` writes no `satp` (see `hal::arch::SwitchPlan`).

pub use crate::hal::arch::{set_kernel_stack_for, switch, IrqSaved, Root, SwitchPlan};

/// `yield_cpu` masks SIE on entry, before any scheduler lock or publication:
/// cooperative yields can enter with SIE set, and masking only at `__switch`
/// would leave the current/selected scheduler state interruptible.
pub const MASK_AT_ENTRY: bool = true;

/// RV64 reports its harts through the SMP and self-test lines instead.
pub const REPORTS_DISPATCH: bool = false;

/// The root for a top-level table at physical address `table` with tag `asid`
/// (`satp` takes the table's page number).
#[inline(always)]
pub fn root_for_table(table: usize, asid: usize) -> Option<Root> {
    Root::new(table >> 12, asid)
}
