// SPDX-License-Identifier: MPL-2.0
//! `irq`: local interrupt delivery.

/// Enable interrupts on this hart: set SPP=1 and SIE=1 in sstatus (0x102).
pub(crate) fn enable() {
    // SAFETY: csrs sstatus SPP|SIE from S-mode — standard interrupt enable.
    unsafe {
        core::arch::asm!("csrs sstatus, {0}", in(reg) 0x102usize);
    }
}

/// Pend a supervisor software interrupt (sip.SSIP) on this hart; it is taken
/// once sstatus.SIE is set again.
pub(crate) fn raise_local_resched() {
    // SAFETY: csrsi on sip.SSIP is permitted from S-mode (RISC-V priv spec §4.1.3).
    unsafe { core::arch::asm!("csrsi sip, 0x2") };
}

/// Pend sip.SSIP on this hart from interrupt context (the waker's form).
pub(crate) fn pend_local_ssip() {
    // SAFETY: csrsi sip.SSIP is permitted from S-mode (RISC-V priv spec §4.1.3).
    unsafe { core::arch::asm!("csrsi sip, 0x2", options(nomem, nostack)) };
}
