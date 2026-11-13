// SPDX-License-Identifier: MPL-2.0
//! `sched` for the 32-bit targets: the scheduler's context-switch seam.
//!
//! These HALs have no private roots and `yield_cpu` never masked interrupts on
//! them, so the interrupt token is empty and the root is uninhabited.

use crate::hal::arch::Context;

/// No interrupt state is captured on these targets (as before the seam).
pub const MASK_AT_ENTRY: bool = false;

/// The dispatch line is an aarch64 and x86_64 report.
pub const REPORTS_DISPATCH: bool = false;

/// No private root exists on these targets.
pub enum Root {}

/// An empty interrupt token: `yield_cpu` never masked on these targets.
#[must_use]
pub struct IrqSaved(core::marker::PhantomData<*mut ()>);

impl IrqSaved {
    #[inline(always)]
    pub fn save() -> Self {
        Self(core::marker::PhantomData)
    }
}

/// One context switch; `root` is always `None`.
#[must_use]
pub struct SwitchPlan {
    pub out: *mut Context,
    pub inc: *const Context,
    pub root: Option<Root>,
    pub irq: IrqSaved,
}

/// Run `plan` through the HAL's plain `Context::switch`.
///
/// # Safety
/// Both pointers must be valid context slots.
#[inline(always)]
pub unsafe fn switch(plan: SwitchPlan) -> IrqSaved {
    let SwitchPlan {
        out,
        inc,
        root,
        irq,
    } = plan;
    if let Some(root) = root {
        match root {}
    }
    // SAFETY: the caller's contract.
    unsafe { Context::switch(out, inc) };
    irq
}

/// Publish the incoming context's kernel stack for trap entry.
#[inline(always)]
pub fn set_kernel_stack_for(inc: &Context) {
    #[cfg(target_arch = "riscv32")]
    crate::hal::arch::set_kernel_stack(inc.sp as usize);
    #[cfg(not(target_arch = "riscv32"))]
    crate::hal::arch::set_kernel_stack(inc.sp);
}
