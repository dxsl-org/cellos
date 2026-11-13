// SPDX-License-Identifier: MPL-2.0
//! `smp`: per-CPU registers and secondary release.

/// `MPIDR_EL1`, this core's affinity.
#[inline(always)]
pub(crate) fn mpidr() -> u64 {
    let mpidr: u64;
    // SAFETY: `MPIDR_EL1` is readable from EL1 and modifies nothing.
    unsafe {
        core::arch::asm!("mrs {}, mpidr_el1", out(reg) mpidr, options(nomem, nostack));
    }
    mpidr
}

/// `SEV`: wake cores parked in WFE.
pub(crate) fn send_event() {
    // SAFETY: SEV only wakes cores parked in WFE.
    unsafe {
        core::arch::asm!("sev", options(nomem, nostack));
    }
}
