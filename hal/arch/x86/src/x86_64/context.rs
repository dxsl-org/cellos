//! x86_64 cooperative registers, migration scratch, and FP control context.
use core::arch::asm;

/// SysV callee-preserved floating-point controls. XMM/x87 data are caller-saved;
/// asynchronous user state lives in the ISR/syscall stack's FXSAVE image.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FpuControl {
    pub mxcsr: u32,
    pub x87_control: u16,
    _reserved: u16,
}

impl FpuControl {
    pub const fn new() -> Self {
        Self {
            mxcsr: 0x1f80,
            x87_control: 0x037f,
            _reserved: 0,
        }
    }
}
impl Default for FpuControl {
    fn default() -> Self {
        Self::new()
    }
}

#[repr(C, align(16))]
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
    /// Suspended user-entry CR3 scratch; moves with the task, not the CPU.
    pub user_cr3: u64,
    pub fp_control: FpuControl,
}

const _: () = {
    use core::mem::{offset_of, size_of};
    assert!(offset_of!(CpuContext, sp) == 48);
    assert!(offset_of!(CpuContext, rip) == 56);
    assert!(offset_of!(CpuContext, kernel_trap_sp) == 64);
    assert!(offset_of!(CpuContext, user_cr3) == 72);
    assert!(offset_of!(CpuContext, fp_control) == 80);
    assert!(size_of::<CpuContext>() == 96);
    assert!(size_of::<FpuControl>() == 8);
    assert!(offset_of!(FpuControl, mxcsr) == 0);
    assert!(offset_of!(FpuControl, x87_control) == 4);
    assert!(core::mem::align_of::<CpuContext>() == 16);
};

unsafe extern "C" {
    fn vi_context_switch_complete();
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
            options(nomem),
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
        options(nomem),
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

/// A private root the switch hands to user entry: the PML4's physical address
/// and its PCID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Root {
    base: core::num::NonZeroUsize,
    asid: usize,
}

impl Root {
    /// `base` is the PML4's physical address. Zero names no root (`None`).
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

/// This CPU's RFLAGS as captured by [`IrqSaved::save`], with interrupts
/// disabled until the token is dropped.
///
/// RFLAGS is not part of a [`CpuContext`]: [`switch`] carries the token across
/// the switch and returns it when this activation resumes, possibly on another
/// CPU, and the caller drops it after its post-resume work.
///
/// Both asm blocks are full compiler barriers (no `nomem`, unlike
/// [`save_and_disable_interrupts`]): no scheduler publication may move out of
/// the masked window.
#[must_use]
pub struct IrqSaved {
    rflags: usize,
    _not_send: core::marker::PhantomData<*mut ()>,
}

impl IrqSaved {
    /// Capture RFLAGS and disable interrupts.
    #[inline(always)]
    pub fn save() -> Self {
        let rflags: usize = unsafe {
            let flags;
            core::arch::asm!("pushfq", "pop {}", "cli", out(reg) flags);
            flags
        };
        Self {
            rflags,
            _not_send: core::marker::PhantomData,
        }
    }
}

impl Drop for IrqSaved {
    #[inline(always)]
    fn drop(&mut self) {
        let outgoing_rflags = self.rflags;
        // SAFETY: captured on this activation by `save`; the flags belong to
        // the suspended context, so IF is never enabled for an activation that
        // entered masked.
        unsafe {
            core::arch::asm!("push {}", "popfq", in(reg) outgoing_rflags);
        }
    }
}

/// One context switch: the outgoing and incoming contexts, the root to hand to
/// user entry, and the interrupt state captured before selection.
///
/// The switch never writes hardware CR3: it writes the user-entry CR3 scratch
/// (`gs:[24]`). `root: None` means "inherit the incoming context's saved
/// `user_cr3`", which equals SAS only because a SAS context saved the SAS CR3
/// at trap entry; a private-root context resumed with `None` returns to its
/// own private root.
#[must_use]
pub struct SwitchPlan {
    pub out: *mut CpuContext,
    pub inc: *const CpuContext,
    pub root: Option<Root>,
    pub irq: IrqSaved,
}

/// Run `plan` through [`switch_with_root`].
///
/// Returns this activation's interrupt token when it is resumed.
///
/// # Safety
/// Both pointers must point to valid, aligned `CpuContext` structs, `plan.irq`
/// must have been saved by this activation, and `plan.root` must name a root
/// this CPU may run.
#[inline(always)]
pub unsafe fn switch(plan: SwitchPlan) -> IrqSaved {
    let SwitchPlan {
        out,
        inc,
        root,
        irq,
    } = plan;
    let (root_pml4, pcid) = root.map_or((0, 0), |root| (root.base(), root.asid()));
    // SAFETY: the caller's contract.
    unsafe { CpuContext::switch_with_root(out, inc, root_pml4, pcid) };
    irq
}

/// Publish the incoming context's fixed syscall-entry stack (TSS.rsp0 and
/// CPU_LOCAL.kernel_rsp). `kernel_trap_sp` is fixed at spawn, unlike the deep
/// cooperative-switch `sp`.
#[inline(always)]
pub fn set_kernel_stack_for(inc: &CpuContext) {
    super::set_kernel_stack(inc.kernel_trap_sp as usize);
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
    // The kernel masks interrupts across selection and this transition. The
    // completion callback may release the outgoing save only on the new stack.
    unsafe {
        asm!(
            "mov [rdi+0*8], r15",  "mov [rdi+1*8], r14",
            "mov [rdi+2*8], r13",  "mov [rdi+3*8], r12",
            "mov [rdi+4*8], rbx",  "mov [rdi+5*8], rbp",
            "mov [rdi+6*8], rsp",
            "lea rax, [rip+99f]",   "mov [rdi+7*8], rax",
            "mov rax, gs:[24]", "mov [rdi+72], rax",
            "stmxcsr [rdi+80]",
            "fnstcw [rdi+84]",
            // Adopt the incoming stack before any call or stack memory access.
            "mov rsp, [rsi+6*8]",
            // Update user CR3 scratch for incoming task: if rdx (explicit root) != 0 use rdx,
            // else use incoming task's saved user_cr3 from [rsi+72].
            // Hardware CR3 remains kernel CR3 for all kernel code and callbacks.
            "test rdx, rdx",
            "cmovz rdx, [rsi+72]",
            "mov gs:[24], rdx",
            // A fresh trap stack and a suspended Rust stack have different
            // alignments. Preserve the exact incoming RSP across the C call.
            "mov rax, rsp",
            "and rsp, -16",
            "sub rsp, 16",
            "mov [rsp], rsi",
            "mov [rsp+8], rax",
            "call {complete}",
            "mov rsi, [rsp]",
            "mov rsp, [rsp+8]",
            // SysV permits data-register clobbers, but controls move with the
            // suspended activation and are restored after the kernel callback.
            "ldmxcsr [rsi+80]",
            "fldcw [rsi+84]",
            "mov r15, [rsi+0*8]",  "mov r14, [rsi+1*8]",
            "mov r13, [rsi+2*8]",  "mov r12, [rsi+3*8]",
            "mov rbx, [rsi+4*8]",  "mov rbp, [rsi+5*8]",
            "jmp [rsi+7*8]",
            "99:",
            ".byte 0xf3, 0x0f, 0x1e, 0xfa",
            in("rdi") old, in("rsi") new, in("rdx") cr3,
            complete = sym vi_context_switch_complete,
            clobber_abi("C"),
        );
    }
}
