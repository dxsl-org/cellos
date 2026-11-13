// SPDX-License-Identifier: MPL-2.0
//! `cpu`: halting this CPU, its stack pointer, and the snapshot stack switch.

/// One step of a terminal halt loop (`wfi`; the caller masks interrupts).
pub(crate) fn halt() {
    // SAFETY: `wfi` idles this hart.
    unsafe {
        core::arch::asm!("wfi", options(nomem, nostack));
    }
}

/// One step of an idle loop that interrupts may leave (`wfi`).
pub(crate) fn idle() {
    // SAFETY: `wfi` idles this hart.
    unsafe { core::arch::asm!("wfi") };
}

/// The stack pointer, for the allocation-failure stack scan.
pub(crate) fn oom_stack_pointer() -> usize {
    let sp: usize;
    // SAFETY: reading `sp` has no side effects.
    unsafe { core::arch::asm!("mv {}, sp", out(reg) sp) };
    sp
}

/// The stack pointer of the running frame, as a virtual address.
#[cfg(any(feature = "snapshot-qualified", test))]
pub(crate) fn stack_pointer() -> usize {
    let sp: usize;
    // SAFETY: reading `sp` has no effect on memory or the control flow.
    unsafe { core::arch::asm!("mv {}, sp", out(reg) sp, options(nomem, nostack)) };
    sp
}

// The snapshot stack switch: `__cellos_snapshot_stack_run(new_sp, run, arg)` keeps the
// old stack pointer in a callee-saved register, moves `sp` to the reserved
// region, calls `run(arg)`, then restores `sp` — so the caller returns normally
// while everything `run` does (its frame, its buffers) is in the region.
#[cfg(any(feature = "snapshot-qualified", test))]
core::arch::global_asm!(
    ".globl __cellos_snapshot_stack_run",
    "__cellos_snapshot_stack_run:",
    "addi sp, sp, -32",
    "sd ra, 0(sp)",
    "sd s0, 8(sp)",
    "mv s0, sp",
    "mv sp, a0",
    "mv t0, a1",
    "mv a0, a2",
    "jalr t0",
    "mv sp, s0",
    "ld s0, 8(sp)",
    "ld ra, 0(sp)",
    "addi sp, sp, 32",
    "ret",
);
