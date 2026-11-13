// SPDX-License-Identifier: MPL-2.0
//! `power`: system shutdown.

/// Power the machine off (`Syscall::Shutdown`). Test images leave QEMU with
/// success instead.
pub(crate) fn shutdown() -> ! {
    #[cfg(feature = "test-hooks")]
    crate::qemu_exit(true);

    #[cfg(not(feature = "test-hooks"))]
    unsafe {
        // PSCI SYSTEM_OFF via HVC (QEMU virt machine default)
        core::arch::asm!(
            "mov x0, #0x0008",
            "movk x0, #0x8400, lsl #16",
            "hvc #0",
            options(noreturn)
        );
    }
}

/// Leave QEMU (test images): semihosting.
#[cfg(feature = "test-hooks")]
pub(crate) fn qemu_exit(success: bool) -> ! {
    use qemu_exit::QEMUExit;
    qemu_exit::AArch64::new().exit(if success { 0 } else { 1 });
}

/// After a kernel panic is printed: nothing (the caller halts this CPU).
#[cfg(not(all(test, not(target_os = "none"))))]
pub(crate) fn reset_after_panic() {}
