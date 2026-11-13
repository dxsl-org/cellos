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
