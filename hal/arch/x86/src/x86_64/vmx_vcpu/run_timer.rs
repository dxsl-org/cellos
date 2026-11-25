// SPDX-License-Identifier: MIT
//! Ending a guest run on time with the VMX-preemption timer (SDM Vol. 3C
//! 26.5.1).
//!
//! The timer counts down in VMX non-root operation, at the TSC rate divided
//! by a power of two the CPU reports, and forces a VM exit at zero. It ends a
//! run when the guest neither exits nor is interrupted, and it needs nothing
//! from the host's own timer: the host tick stays at its rate while a guest
//! can be given a deadline far finer than it. Two things use it: the caller's
//! budget for the run, and the guest's own APIC timer (`lapic`).

use types::ViResult;

use super::super::msr;
use super::super::vmcs::*;

/// The timer of one VMCS.
#[derive(Clone, Copy)]
pub(super) struct RunTimer {
    /// log2 of TSC ticks per timer tick (`IA32_VMX_MISC[4:0]`).
    shift: u8,
}

impl RunTimer {
    /// The timer of a VMCS whose pin-based controls are `pin_ctls`; `None`
    /// when they do not activate it.
    ///
    /// # Safety
    /// CPL 0 on a CPU in VMX operation (`IA32_VMX_MISC` exists).
    pub(super) unsafe fn of(pin_ctls: u32) -> Option<Self> {
        (pin_ctls & PIN_VMX_PREEMPTION_TIMER != 0).then(|| Self {
            shift: (msr::read(MSR_IA32_VMX_MISC) & 0x1F) as u8,
        })
    }

    /// Arm the current VMCS so that the guest leaves `tsc_ticks` from now at
    /// the latest; with `None`, as late as the field allows. One timer tick is
    /// always granted, so every entry lets the guest make progress.
    ///
    /// # Safety
    /// The target VMCS is current.
    pub(super) unsafe fn arm(self, tsc_ticks: Option<u64>) -> ViResult<()> {
        let unbounded = u64::from(u32::MAX);
        let ticks = tsc_ticks.map_or(unbounded, |ticks| (ticks >> self.shift).clamp(1, unbounded));
        vmwrite(VMX_PREEMPTION_TIMER_VALUE, ticks)
    }
}
