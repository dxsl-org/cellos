// SPDX-License-Identifier: MPL-2.0
//! `irq`: local interrupt delivery.

/// Enable interrupts on this hart: clear DAIF.I to unmask IRQs.
pub(crate) fn enable() {
    // SAFETY: unmasking IRQs at EL1/EL2 touches no memory.
    unsafe {
        core::arch::asm!("msr daifclr, #2", options(nomem, nostack));
    }
}
