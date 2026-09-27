//! x86_64 CPU context (callee-saved registers + RSP for cooperative switch).
use core::arch::asm;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct CpuContext {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub rbx: u64,
    pub rbp: u64,
    /// Kernel stack pointer (mapped to RSP; offset 6*8 in the switch asm).
    pub sp: u64,
    /// Resume instruction pointer (mapped to RIP via `jmp`; offset 7*8).
    pub rip: u64,
    /// Fixed syscall-entry RSP = kstack_top − TRAP_FRAME_SIZE.
    /// Set once at spawn; NEVER updated by cooperative switches.
    /// Used by `set_kernel_stack` so CPU_LOCAL.kernel_rsp always points to
    /// the top of a fresh syscall frame, not the deep cooperative-switch RSP
    /// that shrinks every blocking cycle.
    pub kernel_trap_sp: u64,
}

/// Atomically capture RFLAGS and disable interrupts (CLI).
#[inline(always)]
pub fn save_and_disable_interrupts() -> usize {
    let rflags: usize;
    unsafe {
        core::arch::asm!(
            "pushfq",
            "pop {saved}",
            "cli",
            saved = out(reg) rflags,
            options(nomem, nostack),
        );
    }
    rflags
}

/// Restore RFLAGS returned by [`save_and_disable_interrupts`].
///
/// # Safety
/// `rflags` must be a valid RFLAGS state captured on this CPU.
#[inline(always)]
pub unsafe fn restore_sstatus(rflags: usize) {
    core::arch::asm!(
        "push {saved}",
        "popfq",
        saved = in(reg) rflags,
        options(nomem, nostack),
    );
}

impl CpuContext {
    /// Cooperative context switch — associated-function form used by the kernel.
    ///
    /// # Safety
    /// Both pointers must point to valid, aligned `CpuContext` structs.
    #[inline(always)]
    pub unsafe fn switch(old: *mut CpuContext, new: *const CpuContext) {
        // SAFETY: invariant upheld by caller; no root transition.
        unsafe { switch_with_root(old, new, 0, 0) }
    }

    /// Switch contexts, programming a private root between the save and the load.
    ///
    /// `root_pml4 == 0` selects the no-write path (SAS to SAS). Otherwise the CR3
    /// value is composed by the backend — including the PCID decision, so a tag
    /// is only carried when `CR4.PCIDE` makes it legal — and written after the
    /// outgoing context is stored and before the incoming stack is adopted.
    ///
    /// # Safety
    /// Both pointers must point to valid, aligned `CpuContext` structs, and
    /// `root_pml4`/`pcid` must describe a completed root this CPU may run.
    pub unsafe fn switch_with_root(
        old: *mut CpuContext,
        new: *const CpuContext,
        root_pml4: usize,
        pcid: usize,
    ) {
        // SAFETY: invariant upheld by caller.
        unsafe { switch_with_root(old, new, root_pml4, pcid) }
    }
}

/// Cooperative context switch.
///
/// # Safety
/// Both pointers must point to valid, aligned `CpuContext` structs.
pub unsafe fn switch(old: *mut CpuContext, new: *const CpuContext) {
    // SAFETY: caller guarantees valid, aligned CpuContext pointers.
    unsafe { switch_with_root(old, new, 0, 0) }
}

/// Cooperative context switch with a root transition.
///
/// `root_pml4 == 0` means "no write" (SAS to SAS, kernel CR3 already live).
/// Otherwise CR3 is composed by the backend — which drops the PCID when
/// `CR4.PCIDE` is clear — and written between the outgoing save and the incoming
/// load: the outgoing stack is not mapped in the incoming root, so the sequence
/// between them must not touch it (registers only).
///
/// # Safety
/// Both pointers must point to valid, aligned `CpuContext` structs, and
/// `root_pml4`/`pcid` must name a root this CPU may run.
pub unsafe fn switch_with_root(
    old: *mut CpuContext,
    new: *const CpuContext,
    root_pml4: usize,
    pcid: usize,
) {
    let cr3 = if root_pml4 == 0 {
        0
    } else {
        super::domain::cr3_for(root_pml4, pcid, super::domain::pcid_usable())
    };
    // SAFETY: caller guarantees valid, aligned CpuContext pointers.
    //
    // Register discipline: pin `old` → rdi and `new` → rsi (SysV argument
    // registers).  Neither is ever written by the asm body — only their
    // *pointed-to* memory is touched — so both survive intact through the
    // jmp.  Without explicit pins, LLVM may assign `new` to r15/r14/r13/r12/
    // rbx/rbp, which the body overwrites, corrupting the pointer before the
    // final `jmp [rsi+7*8]` and causing a triple-fault (#PF at ~address 0).
    unsafe {
        asm!(
            "mov [rdi+0*8], r15",  "mov [rdi+1*8], r14",
            "mov [rdi+2*8], r13",  "mov [rdi+3*8], r12",
            "mov [rdi+4*8], rbx",  "mov [rdi+5*8], rbp",
            "mov [rdi+6*8], rsp",
            "lea rax, [rip+99f]",   "mov [rdi+7*8], rax",
            // Root transition (rdx = CR3, zero = no write). Registers only:
            // after this point the outgoing stack is unreachable.
            "test rdx, rdx",
            "jz 98f",
            "mov cr3, rdx",
            "98:",
            "mov r15, [rsi+0*8]",  "mov r14, [rsi+1*8]",
            "mov r13, [rsi+2*8]",  "mov r12, [rsi+3*8]",
            "mov rbx, [rsi+4*8]",  "mov rbp, [rsi+5*8]",
            "mov rsp, [rsi+6*8]",
            "jmp [rsi+7*8]",
            "99:",
            in("rdi") old, in("rsi") new, in("rdx") cr3,
            out("rax") _,
        );
    }
}
