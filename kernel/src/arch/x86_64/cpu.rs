// SPDX-License-Identifier: MPL-2.0
//! `cpu`: halting this CPU, its stack pointer, and the snapshot stack switch.

/// One step of a terminal halt loop: `cli; hlt` stops this CPU.
pub(crate) fn halt() {
    // SAFETY: `cli; hlt` stops this CPU; nothing runs afterwards.
    unsafe {
        core::arch::asm!("cli; hlt", options(nomem, nostack));
    }
}

/// One step of an idle loop that interrupts may leave (`hlt`).
pub(crate) fn idle() {
    // SAFETY: `hlt` waits for the next interrupt.
    unsafe { core::arch::asm!("hlt", options(nomem, nostack)) };
}

/// The stack pointer of the running frame, as a virtual address.
#[cfg(any(feature = "snapshot-qualified", test))]
pub(crate) fn stack_pointer() -> usize {
    let sp: usize;
    // SAFETY: reading `rsp` has no effect on memory or the control flow.
    unsafe { core::arch::asm!("mov {}, rsp", out(reg) sp, options(nomem, nostack)) };
    sp
}

// The snapshot stack switch: `__cellos_snapshot_stack_run(new_sp, run, arg)` keeps the
// old stack pointer in a callee-saved register, moves `sp` to the reserved
// region, calls `run(arg)`, then restores `sp` — so the caller returns normally
// while everything `run` does (its frame, its buffers) is in the region.
#[cfg(any(feature = "snapshot-qualified", test))]
core::arch::global_asm!(
    ".text",
    ".globl __cellos_snapshot_stack_run",
    "__cellos_snapshot_stack_run:",
    "push rbp",
    "mov rbp, rsp",
    "mov rsp, rdi",
    "mov rax, rsi",
    "mov rdi, rdx",
    "call rax",
    "mov rsp, rbp",
    "pop rbp",
    "ret",
);
