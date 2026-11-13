// SPDX-License-Identifier: MPL-2.0
//! `cpu`: halting this CPU, its stack pointer, and the snapshot stack switch.

/// One step of a terminal halt loop (`wfi`; the caller masks interrupts).
pub(crate) fn halt() {
    // SAFETY: `wfi` idles this CPU.
    unsafe {
        core::arch::asm!("wfi", options(nomem, nostack));
    }
}

/// One step of an idle loop that interrupts may leave (`wfi`).
pub(crate) fn idle() {
    // SAFETY: `wfi` idles this CPU.
    unsafe { core::arch::asm!("wfi") };
}

/// The stack pointer of the running frame, as a virtual address.
#[cfg(any(feature = "snapshot-qualified", test))]
pub(crate) fn stack_pointer() -> usize {
    let sp: usize;
    // SAFETY: reading `sp` has no effect on memory or the control flow.
    unsafe { core::arch::asm!("mov {}, sp", out(reg) sp, options(nomem, nostack)) };
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
    "stp x29, x30, [sp, #-32]!",
    "str x19, [sp, #16]",
    "mov x29, sp",
    "mov sp, x0",
    "mov x19, x1",
    "mov x0, x2",
    "blr x19",
    "mov sp, x29",
    "ldr x19, [sp, #16]",
    "ldp x29, x30, [sp], #32",
    "ret",
);
