// SPDX-License-Identifier: MPL-2.0
//! `cpu` for the 32-bit targets.

/// One step of a terminal halt loop.
pub(crate) fn halt() {
    #[cfg(target_arch = "x86")]
    // SAFETY: `cli; hlt` stops this hart; nothing runs afterwards.
    unsafe {
        core::arch::asm!("cli; hlt", options(nomem, nostack));
    }
    #[cfg(not(target_arch = "x86"))]
    // SAFETY: `wfi` idles this hart with interrupts masked.
    unsafe {
        core::arch::asm!("wfi", options(nomem, nostack));
    }
}

/// One step of an idle loop that interrupts may leave.
pub(crate) fn idle() {
    unsafe { core::arch::asm!("wfi") };
}

/// No stack switch exists here, so `run_on_stack` cannot stage outside the
/// in-flight stack; the caller's stack-pointer proof refuses the capture.
pub(crate) fn stack_pointer() -> usize {
    0
}
