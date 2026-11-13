// SPDX-License-Identifier: MPL-2.0
//! `power` for the 32-bit targets.

/// `Syscall::Shutdown`: idle forever.
pub(crate) fn shutdown() -> ! {
    loop {
        unsafe {
            core::arch::asm!("wfi", options(nomem, nostack));
        }
    }
}

/// No QEMU exit device on the 32-bit targets: spin so the test times out
/// clearly.
#[cfg(feature = "test-hooks")]
pub(crate) fn qemu_exit(_success: bool) -> ! {
    #[allow(clippy::empty_loop)]
    loop {}
}

/// After a kernel panic is printed: reboot through SBI SRST on riscv32.
#[cfg(not(all(test, not(target_os = "none"))))]
pub(crate) fn reset_after_panic() {
    #[cfg(target_arch = "riscv32")]
    crate::hal::sbi::system_reset(crate::hal::sbi::SBI_RESET_COLD_REBOOT, 0);
}
