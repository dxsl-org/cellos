//! AArch64 CPU context for cooperative context switching.
//!
//! Two switch implementations:
//!   `__switch_el1` — saves/restores elr_el1/spsr_el1 (EL1 normal boot).
//!   `__switch_el2` — saves/restores elr_el2/spsr_el2 (EL2 hypervisor host).
//! Runtime selection via `el2::is_el2()`.

use core::arch::global_asm;

/// Saved CPU context for a kernel-side task.
///
/// Stores the callee-saved registers (x19..x30) plus the stack pointer and
/// the system registers that must survive a context switch.
///
/// At EL1 `elr_el1`/`spsr_el1` hold the real ELR/SPSR values.
/// At EL2 the same fields hold `elr_el2`/`spsr_el2` — the struct is a plain
/// bag of u64 and the field names are irrelevant at the Rust level.
///
/// `sp_el0` is the user-space stack pointer.  AArch64 banks SP_EL0 separately
/// from SP_EL1/SP_EL2, but the CPU does NOT save/restore it on exception
/// entry/exit.  Without explicit save/restore here every `eret` would use the
/// last *other* task's user SP — causing the wrong user stack and eventual guard
/// page faults.
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct CpuContext {
    // Callee-saved per AArch64 ABI.
    pub x19: u64,      // offset 0
    pub x20: u64,      // offset 8
    pub x21: u64,      // offset 16
    pub x22: u64,      // offset 24
    pub x23: u64,      // offset 32
    pub x24: u64,      // offset 40
    pub x25: u64,      // offset 48
    pub x26: u64,      // offset 56
    pub x27: u64,      // offset 64
    pub x28: u64,      // offset 72
    pub x29: u64,      // offset 80  — frame pointer
    pub x30: u64,      // offset 88  — link register (return address)
    pub sp: u64,       // offset 96  — EL1/EL2 kernel stack pointer
    pub elr_el1: u64,  // offset 104 — holds ELR_EL2 at EL2
    pub spsr_el1: u64, // offset 112 — holds SPSR_EL2 at EL2
    pub sp_el0: u64,   // offset 120 — EL0 user-space stack pointer (banked, not auto-saved)
    /// PSTATE.DAIF mask bits (offset 128).
    ///
    /// AArch64 does NOT save DAIF on exception entry or context switch.  Without
    /// explicit save/restore, switching from an IRQ-disabled SVC handler to the
    /// boot idle context leaves DAIF.I=1, causing WFI to sleep forever.
    ///
    /// Saved via `mrs x9, daif`, restored via `msr daif, x9`.
    pub daif: u64, // offset 128
}

/// Atomically capture DAIF and mask IRQs (DAIF.I = 1).
#[inline(always)]
pub fn save_and_disable_interrupts() -> usize {
    let daif: usize;
    unsafe {
        core::arch::asm!(
            "mrs {saved}, daif",
            "msr daifset, #2",
            saved = out(reg) daif,
            options(nostack),
        );
    }
    daif
}
/// Restore a value returned by [`save_and_disable_interrupts`].
///
/// # Safety
/// `daif` must be a valid DAIF state captured on this PE.
#[inline(always)]
pub unsafe fn restore_sstatus(daif: usize) {
    core::arch::asm!(
        "msr daif, {saved}",
        saved = in(reg) daif,
        options(nostack),
    );
}

impl CpuContext {
    /// A context with every field zeroed, usable in a `static`.
    ///
    /// Callers set `sp`, `x30` and `daif`; the remaining fields are the saved
    /// system registers, which a fixture that switches back before returning may
    /// leave zero.
    pub const fn zeroed() -> Self {
        Self {
            x19: 0,
            x20: 0,
            x21: 0,
            x22: 0,
            x23: 0,
            x24: 0,
            x25: 0,
            x26: 0,
            x27: 0,
            x28: 0,
            x29: 0,
            x30: 0,
            sp: 0,
            elr_el1: 0,
            spsr_el1: 0,
            sp_el0: 0,
            daif: 0,
        }
    }

    /// Perform a context switch from `old` to `new`.
    ///
    /// Dispatches to `__switch_el2` when the kernel booted at EL2, otherwise
    /// uses `__switch_el1`.
    ///
    /// # Safety
    /// Both pointers must point to valid, aligned `CpuContext` structs.
    pub unsafe fn switch(old: *mut CpuContext, new: *const CpuContext) {
        // SAFETY: caller's contract; no root transition.
        unsafe { Self::switch_with_root(old, new, 0, 0) }
    }

    /// Switch contexts, programming a private root between the save and the load.
    ///
    /// `root_baddr == 0` selects the no-write path (SAS to SAS). Otherwise the
    /// incoming root is live only after the outgoing context is stored, and the
    /// incoming stack is adopted before any stack access — the ordering the
    /// private-root transition requires.
    ///
    /// # Safety
    /// Both pointers must point to valid, aligned `CpuContext` structs, and
    /// `root_baddr`/`asid` must describe a completed root this hart may run.
    pub unsafe fn switch_with_root(
        old: *mut CpuContext,
        new: *const CpuContext,
        root_baddr: usize,
        asid: usize,
    ) {
        if super::el2::is_el2() {
            // EL2 switches are host-context only: no private roots exist there,
            // and the EL2 stub has no root argument.
            debug_assert_eq!(root_baddr, 0, "private root requested at EL2");
            // SAFETY: same preconditions as __switch_el1; uses EL2 sysregs.
            unsafe {
                __switch_el2(old, new);
            }
        } else {
            let root = if root_baddr == 0 {
                0
            } else {
                super::domain::root_register_value(root_baddr, asid)
            };
            // SAFETY: delegated to the assembly stub below.
            unsafe {
                __switch_el1(old, new, root);
            }
        }
    }
}

/// Free-function alias kept for backwards compatibility.
///
/// # Safety
/// Both pointers must point to valid, aligned `CpuContext` structs.
pub unsafe fn switch(old: *mut CpuContext, new: *const CpuContext) {
    // SAFETY: caller's contract; no root transition.
    unsafe { CpuContext::switch_with_root(old, new, 0, 0) }
}

extern "C" {
    /// EL1 context switch — defined in the global_asm! block below.
    ///
    /// `root` is the TTBR0_EL1 value to program after the outgoing context is
    /// saved and before the incoming context is loaded; zero leaves the register
    /// alone (SAS to SAS, where the kernel root is already live).
    fn __switch_el1(old: *mut CpuContext, new: *const CpuContext, root: usize);
    /// EL2 context switch — defined in el2.rs global_asm! block.
    fn __switch_el2(old: *mut CpuContext, new: *const CpuContext);
}

global_asm!(
    r#"
    .section .text
    .global __switch_el1
    .balign 4
__switch_el1:
    // x0 = old CpuContext*, x1 = new CpuContext*
    // Save callee-saved + sp + link register.
    stp  x19, x20, [x0, #0]
    stp  x21, x22, [x0, #16]
    stp  x23, x24, [x0, #32]
    stp  x25, x26, [x0, #48]
    stp  x27, x28, [x0, #64]
    stp  x29, x30, [x0, #80]
    mov  x9,  sp
    str  x9,       [x0, #96]
    mrs  x9,  elr_el1
    mrs  x10, spsr_el1
    stp  x9,  x10, [x0, #104]
    // SP_EL0 is banked and NOT saved by the CPU on exception entry.
    // Save it explicitly so each task's user-space stack pointer survives
    // across context switches and every eret returns to the correct user SP.
    mrs  x9,  sp_el0
    str  x9,       [x0, #120]
    // Save DAIF so switching from an IRQ-disabled SVC context to the boot idle
    // context restores the idle loop's IRQ-enabled DAIF, keeping WFI functional.
    mrs  x9,  daif
    str  x9,       [x0, #128]

    // Root transition, if the caller asked for one (x2 = TTBR0_EL1 value; zero
    // means "no write", the SAS path). It sits between the save and the load on
    // purpose: the outgoing context is already stored, and the only memory this
    // sequence touches afterwards is the *incoming* context and its stack. The
    // outgoing stack is not mapped in the incoming root, so nothing here may
    // touch it — no calls, no spills, registers only.
    cbz  x2,  1f
    msr  ttbr0_el1, x2
    isb
1:

    // Restore new context.
    ldp  x19, x20, [x1, #0]
    ldp  x21, x22, [x1, #16]
    ldp  x23, x24, [x1, #32]
    ldp  x25, x26, [x1, #48]
    ldp  x27, x28, [x1, #64]
    ldp  x29, x30, [x1, #80]
    ldr  x9,       [x1, #96]
    mov  sp,  x9
    ldp  x9,  x10, [x1, #104]
    msr  elr_el1,  x9
    msr  spsr_el1, x10
    ldr  x9,       [x1, #120]
    msr  sp_el0,   x9
    // Restore DAIF: re-enables IRQs when loading boot context (DAIF.I=0),
    // keeps IRQs disabled when loading a task context from a SVC handler.
    ldr  x9,       [x1, #128]
    msr  daif,     x9

    ret
    "#
);
