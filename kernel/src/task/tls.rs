//! Per-task user thread pointer (the TLS base primitive, ADR-0018 §2.1).
//!
//! The kernel owns exactly one word per task and installs it as the *user* thread
//! pointer. The value is opaque: a cell allocates its own TLS blocks and tells the
//! kernel where they are. Nothing here parses, dereferences, or bounds-checks the
//! base — a bad value is the cell's own fault and is contained by its tier (LBI on
//! Tier 1, an MMU fault on Tier 2), which is why this syscall carries no authority
//! and needs no allowlist bit.
//!
//! The carrier differs per architecture, and each choice is deliberate:
//!
//! * **riscv64** — the user `tp` (x4) is part of the trap frame. `HART_TRAP_ENTRY`
//!   parks the user's `tp` in the frame and `__trap_exit` restores it, so writing
//!   the frame is enough; no switch work is needed. The kernel's own `tp` is a
//!   different value (the HartLocal pointer, reloaded on every U→S transition), so
//!   the two never collide.
//! * **aarch64** — `TPIDR_EL0` is not banked by the exception model and is not part
//!   of the trap frame, so the kernel writes it on every resume and when the cell
//!   sets a base. `TPIDR_EL1` (kernel stack) is untouched.
//! * **x86_64** — `FS_BASE` (`IA32_FS_BASE`) is written on resume and on set; the
//!   `GS_BASE`/`KERNEL_GS_BASE` pair stays kernel context state.
//!
//! Contract: the kernel owns this register. A cell must use the `SetTlsBase`
//! syscall rather than writing the register directly, because a direct user write
//! is overwritten on the next resume. `SetTlsBase` returns the previous base, which
//! is also the only supported way for a cell to read it back.

use super::tcb::Task;

/// Store the caller's base and make it effective for the current user return.
pub(crate) fn set_base(task: &mut Task, base: usize) {
    task.tls_base = base;
    #[cfg(target_arch = "riscv64")]
    write_frame_tp(task, base);
    #[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
    write_user_thread_pointer(base);
}

/// Install the selected task's base before it returns to user mode.
///
/// The scheduler publishes the base while holding `SCHEDULER`. This runs both
/// before a raw switch (so a fresh task's direct jump to `__trap_exit` has the
/// right carrier) and after a switched-back context resumes. RISC-V carries the
/// value in the task trap frame instead.
pub(crate) fn install_published_base() {
    #[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
    write_user_thread_pointer(super::hart_local::current_tls_base());
}

/// Reinstall a switched-back task's base after its raw context switch returns.
#[inline]
pub(crate) fn apply_on_resume() {
    install_published_base();
}

/// Give a newly spawned thread the creator's base, unless the creator had none.
///
/// This is deliberately distinct from [`set_base`]: the new task is not the
/// running task, so installing an AArch64/x86 user pointer here would corrupt
/// the creator's hardware register. On RISC-V the fully primed trap frame owns
/// the register; other architectures install the stored base before the raw
/// switch that enters the child.
pub(crate) fn inherit_from(task: &mut Task, parent_base: usize) {
    if parent_base == 0 {
        return;
    }
    task.tls_base = parent_base;
    #[cfg(target_arch = "riscv64")]
    write_frame_tp(task, parent_base);
}

/// Write the user `tp` slot of a task's trap frame.
///
/// The frame sits at the top of the task's kernel stack in every case: the spawn
/// path primes it there (`task.rs`: `kernel_stack.top - TRAP_FRAME_SIZE`), and a
/// U→S trap rebuilds it at the same address. For a task currently in a syscall
/// this is the live frame, so the write takes effect on the trap return.
#[cfg(target_arch = "riscv64")]
fn write_frame_tp(task: &Task, base: usize) {
    let Some(stack) = task.kernel_stack.as_ref() else {
        return;
    };
    let frame = (stack.top - super::TRAP_FRAME_SIZE) as *mut crate::hal::arch::ViTrapFrame;
    // SAFETY: the address is the task's own trap frame inside its kernel stack,
    // which is allocated and owned by this task for its whole lifetime. x4 is the
    // thread pointer slot; `__trap_exit` loads it with `ld x4, 4*8(sp)`.
    unsafe {
        (*frame).regs[4] = base;
    }
}

/// Write the architecture's user thread pointer register (aarch64/x86_64).
#[cfg(target_arch = "aarch64")]
fn write_user_thread_pointer(base: usize) {
    // SAFETY: TPIDR_EL0 is the EL0-visible thread pointer; writing it from EL1 is
    // always permitted and touches no kernel state (the kernel uses TPIDR_EL1).
    unsafe {
        core::arch::asm!("msr tpidr_el0, {0}", in(reg) base, options(nomem, nostack));
    }
}

#[cfg(target_arch = "x86_64")]
fn write_user_thread_pointer(base: usize) {
    const IA32_FS_BASE: u32 = 0xC000_0100;
    // SAFETY: WRMSR of IA32_FS_BASE from ring 0 is always permitted; it sets the
    // user-mode FS segment base and leaves the kernel's GS-based per-CPU state
    // (GS_BASE/KERNEL_GS_BASE) untouched.
    unsafe {
        core::arch::asm!(
            "wrmsr",
            in("ecx") IA32_FS_BASE,
            in("eax") base as u32,
            in("edx") (base >> 32) as u32,
            options(nomem, nostack)
        );
    }
}
