// SPDX-License-Identifier: MPL-2.0
//! `power`: system shutdown and the G1 private lane's QEMU exits.

/// `Syscall::Shutdown` on x86 halts this CPU for good.
pub(crate) fn shutdown() -> ! {
    loop {
        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack));
        }
    }
}

/// G1 failure: mask interrupts, write `value` to QEMU's `isa-debug-exit`
/// (port 0xf4), then halt.
#[cfg(feature = "g1-cpu-accounting")]
pub(crate) fn g1_abort(value: u32) -> ! {
    unsafe {
        core::arch::asm!("cli");
    }
    unsafe {
        core::arch::asm!("out dx, eax", in("dx") 0xf4u16, in("eax") value);
    }
    loop {
        unsafe {
            core::arch::asm!("hlt");
        }
    }
}

/// G1 cutoff reached: write the pass code to `isa-debug-exit`, then halt.
#[cfg(feature = "g1-cpu-accounting")]
pub(crate) fn g1_cutoff() -> ! {
    unsafe {
        core::arch::asm!("out dx, eax", in("dx") 0xf4u16, in("eax") 0x10u32);
    }
    loop {
        unsafe {
            core::arch::asm!("cli", "hlt");
        }
    }
}
