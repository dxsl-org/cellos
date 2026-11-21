//! AArch64 CPU context for cooperative context switching.
//!
//! Two switch implementations:
//!   `__switch_el1` — saves/restores elr_el1/spsr_el1 (EL1 normal boot).
//!   `__switch_el2` — saves/restores elr_el2/spsr_el2 (EL2 hypervisor host).
//! Runtime selection via `el2::is_el2()`. Both save and restore the task's
//! FP/SIMD register file (`q0`-`q31`, `FPCR`, `FPSR`).

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
///
/// `fpcr`/`fpsr`/`v` are the task's FP/SIMD state. EL0 runs with FP/SIMD
/// untrapped and std Cells are built with `+neon`, while trap entry saves only
/// the general registers; the kernel is softfloat and never touches the
/// vector file, so the switch is the only point where it changes owner.
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
    pub fpcr: u64,     // offset 136
    pub fpsr: u64,     // offset 144
    /// `q0`-`q31` (offset 160; 16-byte aligned for `stp`/`ldp` of Q registers).
    pub v: [u128; 32],
}

// The switch stubs (here and in el2.rs) address these fields by literal offset.
const _: () = {
    assert!(core::mem::offset_of!(CpuContext, sp) == 96);
    assert!(core::mem::offset_of!(CpuContext, daif) == 128);
    assert!(core::mem::offset_of!(CpuContext, fpcr) == 136);
    assert!(core::mem::offset_of!(CpuContext, fpsr) == 144);
    assert!(core::mem::offset_of!(CpuContext, v) == 160);
    assert!(core::mem::size_of::<CpuContext>() == 672);
};

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
            fpcr: 0,
            fpsr: 0,
            v: [0; 32],
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
    ///
    /// # Panics
    /// At EL2 when `root_baddr != 0`. EL2 runs host contexts only and the EL2
    /// stub cannot program a root, so continuing would run the incoming task
    /// under the wrong translation root. Tier 2 admission is refused at EL2
    /// (`domain_admission::enable_for_boot`), so this is an invariant violation.
    pub unsafe fn switch_with_root(
        old: *mut CpuContext,
        new: *const CpuContext,
        root_baddr: usize,
        asid: usize,
    ) {
        if super::el2::is_el2() {
            assert_eq!(root_baddr, 0, "private root requested at EL2");
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

/// A private root the switch programs between the outgoing save and the
/// incoming load: the root table's physical address and its ASID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Root {
    base: core::num::NonZeroUsize,
    asid: usize,
}

impl Root {
    /// `base` is the root table's physical address. Zero names no root
    /// (`None`): the switch's no-write path.
    pub fn new(base: usize, asid: usize) -> Option<Self> {
        core::num::NonZeroUsize::new(base).map(|base| Self { base, asid })
    }

    pub fn base(self) -> usize {
        self.base.get()
    }

    pub fn asid(self) -> usize {
        self.asid
    }
}

/// The DAIF of the activation that called [`IrqSaved::save`], with IRQs masked
/// until the token is dropped.
///
/// [`switch`] carries the token across the switch and returns it when this
/// activation resumes, possibly on another PE; the incoming context restores
/// its own DAIF, so the caller drops the returned token after its post-resume
/// work.
#[must_use]
pub struct IrqSaved {
    daif: usize,
    _not_send: core::marker::PhantomData<*mut ()>,
}

impl IrqSaved {
    /// Capture DAIF and mask IRQs ([`save_and_disable_interrupts`]).
    #[inline(always)]
    pub fn save() -> Self {
        Self {
            daif: save_and_disable_interrupts(),
            _not_send: core::marker::PhantomData,
        }
    }
}

impl Drop for IrqSaved {
    #[inline(always)]
    fn drop(&mut self) {
        // SAFETY: captured on this activation by `save`; the DAIF belongs to
        // the suspended context, so IRQs are never unmasked for an activation
        // that entered masked.
        unsafe { restore_sstatus(self.daif) }
    }
}

/// One context switch: the outgoing and incoming contexts, the root to program
/// between them, and the interrupt state captured before selection.
///
/// `root: None` writes no `TTBR0_EL1`: after trap entry the kernel root is
/// live, so this is the SAS-to-SAS path. At EL2 the root must be `None`.
#[must_use]
pub struct SwitchPlan {
    pub out: *mut CpuContext,
    pub inc: *const CpuContext,
    pub root: Option<Root>,
    pub irq: IrqSaved,
}

/// Run `plan` through `__switch_el1` (or `__switch_el2` at EL2).
///
/// Returns this activation's interrupt token when it is resumed.
///
/// # Safety
/// Both pointers must point to valid, aligned `CpuContext` structs, `plan.irq`
/// must have been saved on this PE, and `plan.root` must name a completed root
/// this PE may run.
#[inline(always)]
pub unsafe fn switch(plan: SwitchPlan) -> IrqSaved {
    let SwitchPlan {
        out,
        inc,
        root,
        irq,
    } = plan;
    let (root_baddr, asid) = root.map_or((0, 0), |root| (root.base(), root.asid()));
    // SAFETY: the caller's contract.
    unsafe { CpuContext::switch_with_root(out, inc, root_baddr, asid) };
    irq
}

/// Publish the incoming context's kernel stack (TPIDR_EL1) for trap entry.
#[inline(always)]
pub fn set_kernel_stack_for(inc: &CpuContext) {
    super::set_kernel_stack(inc.sp as usize);
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

// Both switch stubs call the kernel's `vi_context_switch_complete` (an
// `extern "C"`, `#[no_mangle]` symbol) on the incoming stack after the outgoing
// Context is completely stored. That call is the only proof the scheduler has
// that the outgoing Context may run elsewhere: until it returns, the outgoing
// task's ready-queue entry stays guarded against a peer hart's dispatch.
// Contract of the call site: IRQs still masked (DAIF is restored last), the
// incoming `sp` adopted, `x29 = 0` so no frame chain reaches the outgoing stack
// (unmapped under a private incoming root), and the incoming Context pointer
// held in callee-saved x19 across the call; every incoming register is loaded
// only afterwards.
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
    // FP/SIMD file: offsets 136 (FPCR), 144 (FPSR), 160.. (q0-q31). The kernel
    // is softfloat, so these still hold the outgoing task's EL0 values here.
    // `.arch_extension fp`: the kernel target assembles without FP.
    .arch_extension fp
    mrs  x9,  fpcr
    mrs  x10, fpsr
    stp  x9,  x10, [x0, #136]
    stp  q0,  q1,  [x0, #160]
    stp  q2,  q3,  [x0, #192]
    stp  q4,  q5,  [x0, #224]
    stp  q6,  q7,  [x0, #256]
    stp  q8,  q9,  [x0, #288]
    stp  q10, q11, [x0, #320]
    stp  q12, q13, [x0, #352]
    stp  q14, q15, [x0, #384]
    stp  q16, q17, [x0, #416]
    stp  q18, q19, [x0, #448]
    stp  q20, q21, [x0, #480]
    stp  q22, q23, [x0, #512]
    stp  q24, q25, [x0, #544]
    stp  q26, q27, [x0, #576]
    stp  q28, q29, [x0, #608]
    stp  q30, q31, [x0, #640]

    // Root transition, if the caller asked for one (x2 = TTBR0_EL1 value; zero
    // means "no write", the SAS path). It sits between the save and the load on
    // purpose: the outgoing context is already stored, and the only memory this
    // sequence touches afterwards is the *incoming* context and its stack. The
    // outgoing stack is not mapped in the incoming root, so nothing here may
    // touch it — registers only until the incoming stack is adopted.
    cbz  x2,  1f
    msr  ttbr0_el1, x2
    isb
1:

    // Adopt the incoming stack, then publish switch completion from it.
    mov  x19, x1
    ldr  x9,       [x19, #96]
    mov  sp,  x9
    mov  x29, xzr
    bl   vi_context_switch_complete

    // Restore new context after the completion callback.
    mov  x9,  x19
    ldp  x10, x11, [x9, #136]
    msr  fpcr, x10
    msr  fpsr, x11
    ldp  q0,  q1,  [x9, #160]
    ldp  q2,  q3,  [x9, #192]
    ldp  q4,  q5,  [x9, #224]
    ldp  q6,  q7,  [x9, #256]
    ldp  q8,  q9,  [x9, #288]
    ldp  q10, q11, [x9, #320]
    ldp  q12, q13, [x9, #352]
    ldp  q14, q15, [x9, #384]
    ldp  q16, q17, [x9, #416]
    ldp  q18, q19, [x9, #448]
    ldp  q20, q21, [x9, #480]
    ldp  q22, q23, [x9, #512]
    ldp  q24, q25, [x9, #544]
    ldp  q26, q27, [x9, #576]
    ldp  q28, q29, [x9, #608]
    ldp  q30, q31, [x9, #640]
    ldp  x19, x20, [x9, #0]
    ldp  x21, x22, [x9, #16]
    ldp  x23, x24, [x9, #32]
    ldp  x25, x26, [x9, #48]
    ldp  x27, x28, [x9, #64]
    ldp  x29, x30, [x9, #80]
    ldp  x10, x11, [x9, #104]
    msr  elr_el1,  x10
    msr  spsr_el1, x11
    ldr  x10,      [x9, #120]
    msr  sp_el0,   x10
    // Restore DAIF: re-enables IRQs when loading boot context (DAIF.I=0),
    // keeps IRQs disabled when loading a task context from a SVC handler.
    ldr  x10,      [x9, #128]
    msr  daif,     x10

    ret
    "#
);
