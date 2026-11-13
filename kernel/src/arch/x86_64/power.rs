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

/// Leave QEMU (test images): `isa-debug-exit` at port 0xF4.
#[cfg(feature = "test-hooks")]
pub(crate) fn qemu_exit(success: bool) -> ! {
    use qemu_exit::QEMUExit;
    // SAFETY: 0xF4 is the QEMU `isa-debug-exit` I/O port wired in the x86
    // test harness (`-device isa-debug-exit,iobase=0xf4`); writing it exits
    // QEMU. Constructing the port handle is `unsafe` (arbitrary port I/O).
    unsafe {
        qemu_exit::X86::new(0xF4, 0).exit(if success { 0 } else { 1 });
    }
}

/// After a kernel panic is printed: nothing (the caller halts this CPU).
#[cfg(not(all(test, not(target_os = "none"))))]
pub(crate) fn reset_after_panic() {}
