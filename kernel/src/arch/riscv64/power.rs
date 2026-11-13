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
