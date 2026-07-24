//! SVM world-switch: the `VMRUN` coroutine stub (Tier 3b P03).
//!
//! `svm_vmrun` enters the guest and returns only on `#VMEXIT` (which resumes at
//! the instruction after `VMRUN` — no separate trap vector, unlike ARM's EL2
//! trampoline). The stub hand-manages exactly the state `VMRUN`/`#VMEXIT` do
//! **not** touch automatically (AMD APM §15.5):
//!
//! - **GPRs except RAX/RSP** — loaded from / saved to the caller's `gpr[16]`
//!   array (RAX and RSP live in the VMCB state-save area and are managed by the
//!   Rust caller through VMCB fields).
//! - **FS/GS/KernelGSBase, syscall MSRs (LSTAR/STAR/CSTAR/SFMASK), TR/LDTR** —
//!   saved via `VMSAVE` to a host-save VMCB and reloaded via `VMLOAD` after the
//!   guest exits. **This is the GS.base leak fence**: Cellos reads CPU-local via
//!   `gs:`, so between `VMRUN` return and the host `VMLOAD` the code must touch
//!   NO `gs:`-relative memory and interrupts must stay masked (the caller runs
//!   with IF=0; the stub's own `sti` sits in the STI shadow of `VMRUN` and
//!   `#VMEXIT` clears IF in hardware, so no host handler ever runs inside the
//!   fence — see the inline comment at the `sti`).
//!
//! # gpr[] index = x86 register number
//! 0=RAX 1=RCX 2=RDX 3=RBX 4=RSP 5=RBP 6=RSI 7=RDI 8..15=R8..R15.
//! Slots 0 (RAX) and 4 (RSP) are VMCB-managed and untouched by this stub.

use core::arch::global_asm;

extern "C" {
    /// Enter the guest via `VMRUN` and return on `#VMEXIT`.
    ///
    /// # Arguments (SysV: rdi, rsi, rdx)
    /// * `guest_vmcb_pa` — physical address of the guest VMCB (4 KiB-aligned).
    /// * `gpr_ptr` — pointer to the caller's `[u64; 16]` guest-GPR array.
    /// * `host_vmcb_pa` — physical address of the host-save VMCB (VMSAVE target).
    ///
    /// # Safety
    /// SVM root operation must be active (`EFER.SVME=1`), both VMCB frames valid
    /// and kernel-owned, the guest VMCB fully initialised, and interrupts masked
    /// (IF=0) for the whole call — an interrupt in the VMRUN→VMLOAD window would
    /// read the guest's `gs:` base.
    pub fn svm_vmrun(guest_vmcb_pa: u64, gpr_ptr: *mut u64, host_vmcb_pa: u64);
}

global_asm!(
    ".section .text, \"ax\"",
    ".global svm_vmrun",
    "svm_vmrun:",
    ".byte 0xF3, 0x0F, 0x1E, 0xFA", // ENDBR64 (CET-IBT landing pad)
    // Preserve callee-saved host registers for the Rust caller.
    "push rbp",
    "push rbx",
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    // Stash the three args on the stack: after these pushes
    //   [rsp]    = gpr_ptr        (rsi)
    //   [rsp+8]  = guest_vmcb_pa  (rdi)
    //   [rsp+16] = host_vmcb_pa   (rdx)
    "push rdx",
    "push rdi",
    "push rsi",
    // GIF=0: full fence (INTR/NMI/SMI) around host-state manipulation. #VMEXIT
    // also leaves GIF=0, so without the STGI at the end of this stub the host
    // would never take another interrupt after the first exit.
    "clgi",
    // Save host FS/GS/KernelGSBase/syscall-MSRs/TR/LDTR → host-save VMCB.
    "mov rax, [rsp+16]",
    "vmsave", // implicit operand = rax
    // Load guest GPRs (all except RAX/RSP) from the gpr array.
    "mov rax, [rsp]", // rax = gpr_ptr
    "mov rcx, [rax + 8]",
    "mov rdx, [rax + 16]",
    "mov rbx, [rax + 24]",
    "mov rbp, [rax + 40]",
    "mov rsi, [rax + 48]",
    "mov rdi, [rax + 56]",
    "mov r8,  [rax + 64]",
    "mov r9,  [rax + 72]",
    "mov r10, [rax + 80]",
    "mov r11, [rax + 88]",
    "mov r12, [rax + 96]",
    "mov r13, [rax + 104]",
    "mov r14, [rax + 112]",
    "mov r15, [rax + 120]",
    // Load guest FS/GS/... then run. rax must be guest_vmcb_pa for both.
    "mov rax, [rsp+8]",
    "vmload", // guest FS/GS/KernelGSBase/... ← guest VMCB
    // STI directly before VMRUN: with V_INTR_MASKING set, the host RFLAGS.IF
    // snapshot at VMRUN (HIF) gates whether a physical interrupt (the host
    // scheduler tick) is recognised in the guest and forces a #VMEXIT_INTR.
    // Without it, a guest spin loop containing no intercepted instruction
    // (e.g. Linux's calibrate_delay `while (ticks == jiffies);`) runs
    // unpreemptible forever and freezes the whole host. GIF is 0 here, so the
    // STI delivers nothing; VMRUN sets GIF=1 for the guest.
    "sti",
    "vmrun", // ── GUEST RUNS ── returns here on #VMEXIT (GIF=0 again)
    // Drop IF before anything else: #VMEXIT reloads host RFLAGS from the host
    // save area (IF=1 from the STI above) but leaves GIF=0, so this CLI runs
    // before any interrupt can be taken.
    "cli",
    // On return: rax=host RAX (from HSAVE), rbx..r15=GUEST values, gs=GUEST base.
    // FIRST persist the guest's VMSAVE-managed state (FS/GS/KernelGSBase, the
    // syscall MSRs, TR/LDTR) back into the guest VMCB — VMRUN does NOT save these
    // automatically, so without this the guest's `wrmsr GS_BASE` (per-CPU setup)
    // is lost on the next VMLOAD and every GS-relative access faults at CR2=off.
    "mov rax, [rsp+8]", // rax = guest_vmcb_pa
    "vmsave",           // guest FS/GS/KernelGSBase/... → guest VMCB
    // Save guest GPRs back. rax := gpr_ptr (clobbering host RAX is fine — the
    // Rust caller reads guest RAX from the VMCB, not from here). NO gs: access
    // is permitted until the host VMLOAD below.
    "mov rax, [rsp]", // rax = gpr_ptr
    "mov [rax + 8],   rcx",
    "mov [rax + 16],  rdx",
    "mov [rax + 24],  rbx",
    "mov [rax + 40],  rbp",
    "mov [rax + 48],  rsi",
    "mov [rax + 56],  rdi",
    "mov [rax + 64],  r8",
    "mov [rax + 72],  r9",
    "mov [rax + 80],  r10",
    "mov [rax + 88],  r11",
    "mov [rax + 96],  r12",
    "mov [rax + 104], r13",
    "mov [rax + 112], r14",
    "mov [rax + 120], r15",
    // Restore host FS/GS/KernelGSBase/syscall-MSRs ← host-save VMCB. gs: valid again.
    "mov rax, [rsp+16]",
    "vmload",
    // GIF=1: re-open host interrupt recognition (IF is 0, so any pending host
    // tick stays held until the caller — or the syscall return — re-enables IF
    // outside the VM-registry lock).
    "stgi",
    // Pop the arg slots + callee-saved and return.
    "add rsp, 24",
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop rbx",
    "pop rbp",
    "ret",
);
