// SPDX-License-Identifier: MPL-2.0
//! `power`: system shutdown.

/// Power the machine off (`Syscall::Shutdown`). Test images leave QEMU with
/// success instead.
pub(crate) fn shutdown() -> ! {
    #[cfg(feature = "test-hooks")]
    crate::qemu_exit(true);

    #[cfg(not(feature = "test-hooks"))]
    unsafe {
        // SAFETY: ecall traps to OpenSBI which powers off QEMU; no return.
        core::arch::asm!(
            "li a7, 0x53525354", // SBI_EXT_SRST
            "li a6, 0",          // fid = SYSTEM_RESET
            "li a0, 0",          // reset_type = Shutdown
            "li a1, 0",          // reset_reason = NoReason
            "ecall",
            options(noreturn)
        );
    }
}

/// Leave QEMU (test images): the SiFive test device at 0x100000.
#[cfg(feature = "test-hooks")]
pub(crate) fn qemu_exit(success: bool) -> ! {
    use qemu_exit::QEMUExit;
    // SAFETY: 0x100000 is the SiFive test device address on the QEMU `virt`
    // machine used by this test-only exit path.
    unsafe {
        qemu_exit::RISCV64::new(0x100000).exit(if success { 0 } else { 1 });
    }
}

/// After a kernel panic is printed: reboot through SBI SRST.
#[cfg(not(all(test, not(target_os = "none"))))]
pub(crate) fn reset_after_panic() {
    crate::hal::sbi::system_reset(crate::hal::sbi::SBI_RESET_COLD_REBOOT, 0);
}
