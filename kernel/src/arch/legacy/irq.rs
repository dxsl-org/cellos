// SPDX-License-Identifier: MPL-2.0
//! `irq` for the 32-bit targets.

/// Enable interrupts on this hart.
pub(crate) fn enable() {
    #[cfg(target_arch = "riscv32")]
    // SAFETY: csrs sstatus SPP|SIE from S-mode — standard interrupt enable.
    unsafe {
        core::arch::asm!("csrs sstatus, {0}", in(reg) 0x102usize);
    }
    // x86_32, AArch32: use the Arch trait's enable_interrupts().
    #[cfg(any(target_arch = "x86", target_arch = "arm"))]
    {
        use crate::hal::Arch;
        crate::hal::ARCH.enable_interrupts();
    }
}
