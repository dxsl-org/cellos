//! SVM world-switch: the `VMRUN` coroutine stub (Tier 3 P03).
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
//! - **XCR0 / PKRU** (both backends) and, on VMX, **CR8 (TPR)**,
//!   **IA32_KERNEL_GS_BASE** and the **SYSCALL MSRs (STAR/LSTAR/CSTAR/SFMASK)**
//!   — no exit reloads them, so the Rust wrappers [`svm_vmrun`] / [`vmx_vmrun`]
//!   snapshot the host values before entry and restore them right after the
//!   exit (see `host_ext_state`). SVM's KernelGSBase and SYSCALL MSRs are
//!   already covered by the VMSAVE/VMLOAD pair.
//!
//! # gpr[] index = x86 register number
//! 0=RAX 1=RCX 2=RDX 3=RBX 4=RSP 5=RBP 6=RSI 7=RDI 8..15=R8..R15.
//! Slots 0 (RAX) and 4 (RSP) are VMCB-managed and untouched by this stub.

use core::arch::global_asm;

mod guest_fpu;
mod host_ext_state;
pub use guest_fpu::GuestFpu;
use host_ext_state::HostExtState;

use super::msr;

const MSR_KERNEL_GS_BASE: u32 = 0xC000_0102;

extern "C" {
    fn svm_world_switch(guest_vmcb_pa: u64, gpr_ptr: *mut u64, host_vmcb_pa: u64);
    fn vmx_world_switch(gpr_ptr: *mut u64, launched: u64) -> u32;
}

/// Enter the guest via `VMRUN` and return on `#VMEXIT`.
///
/// `guest_pkru` is the vCPU's PKRU: loaded before entry, refreshed from the
/// register at exit, after which the host PKRU/XCR0 are back in place.
/// `guest_fpu` is its x87/SSE image, loaded before entry and saved at exit.
///
/// # Arguments
/// * `guest_vmcb_pa` — physical address of the guest VMCB (4 KiB-aligned).
/// * `gpr_ptr` — pointer to the caller's `[u64; 16]` guest-GPR array.
/// * `host_vmcb_pa` — physical address of the host-save VMCB (VMSAVE target).
///
/// # Safety
/// SVM root operation must be active (`EFER.SVME=1`), both VMCB frames valid
/// and kernel-owned, the guest VMCB fully initialised, and interrupts masked
/// (IF=0) for the whole call — an interrupt in the VMRUN→VMLOAD window would
/// read the guest's `gs:` base.
#[inline]
pub unsafe fn svm_vmrun(
    guest_vmcb_pa: u64,
    gpr_ptr: *mut u64,
    host_vmcb_pa: u64,
    guest_pkru: &mut u32,
    guest_fpu: &mut GuestFpu,
) {
    let host = HostExtState::enter(*guest_pkru);
    guest_fpu.load();
    svm_world_switch(guest_vmcb_pa, gpr_ptr, host_vmcb_pa);
    guest_fpu.save();
    host.exit(guest_pkru);
}

/// The SYSCALL MSRs, in the order [`VmxGuestRegs::syscall`] stores them.
#[derive(Clone, Copy)]
pub enum SyscallMsr {
    Star = 0,
    Lstar = 1,
    Cstar = 2,
    Sfmask = 3,
}

const SYSCALL_MSRS: [u32; 4] = [0xC000_0081, 0xC000_0082, 0xC000_0083, 0xC000_0084];

/// Guest values of the registers a VMX exit leaves holding guest state.
#[derive(Default)]
pub struct VmxGuestRegs {
    pub pkru: u32,
    /// The guest's `swapgs` partner; without the swap-back the VMM cell would
    /// resume with a guest-chosen user GS base.
    pub kernel_gs_base: u64,
    /// Guest STAR/LSTAR/CSTAR/SFMASK, indexed by [`SyscallMsr`]. VMX switches
    /// none of them, and the guest writes them only through emulated `WRMSR`
    /// (no MSR bitmap), so they are loaded before entry and never read back.
    pub syscall: [u64; 4],
    /// x87/SSE image, loaded before entry and saved at exit.
    pub fpu: GuestFpu,
}

/// Enter the VMX guest via `vmlaunch` (if `launched == 0`) or `vmresume`.
///
/// Returns 0 on VM exit, 1 on VMfailInvalid, 2 on VMfailValid. The host's
/// PKRU, XCR0, KERNEL_GS_BASE, SYSCALL MSRs and CR8 are identical before and
/// after the call (its x87/SSE registers are not: the syscall path restores
/// them); `guest` carries the guest's PKRU/KERNEL_GS_BASE/FPU across exits. CR8 is restored only as a backstop: the VMCS sets CR8-load/store
/// exiting, so the guest never reaches the physical TPR.
///
/// # Safety
/// VMX root operation, this vCPU's VMCS current and fully initialised
/// (host-state fields describing the *current* CPU), `gpr_ptr` a live
/// `[u64; 16]`, and IF=0 for the whole call: between the SYSCALL-MSR load and
/// the restore, a host `syscall` would enter the guest's LSTAR.
#[inline]
pub unsafe fn vmx_vmrun(gpr_ptr: *mut u64, launched: u64, guest: &mut VmxGuestRegs) -> u32 {
    let host_tpr: u64;
    core::arch::asm!("mov {}, cr8", out(reg) host_tpr, options(nomem, nostack, preserves_flags));
    let host_kgs = msr::read(MSR_KERNEL_GS_BASE);
    if guest.kernel_gs_base != host_kgs {
        msr::write(MSR_KERNEL_GS_BASE, guest.kernel_gs_base);
    }
    let mut host_syscall = [0u64; 4];
    for (slot, &index) in SYSCALL_MSRS.iter().enumerate() {
        host_syscall[slot] = msr::read(index);
        if guest.syscall[slot] != host_syscall[slot] {
            msr::write(index, guest.syscall[slot]);
        }
    }
    let host = HostExtState::enter(guest.pkru);
    guest.fpu.load();
    let status = vmx_world_switch(gpr_ptr, launched);
    guest.fpu.save();
    host.exit(&mut guest.pkru);
    for (slot, &index) in SYSCALL_MSRS.iter().enumerate() {
        if guest.syscall[slot] != host_syscall[slot] {
            msr::write(index, host_syscall[slot]);
        }
    }
    let kgs = msr::read(MSR_KERNEL_GS_BASE);
    guest.kernel_gs_base = kgs;
    if kgs != host_kgs {
        msr::write(MSR_KERNEL_GS_BASE, host_kgs);
    }
    let tpr: u64;
    core::arch::asm!("mov {}, cr8", out(reg) tpr, options(nomem, nostack, preserves_flags));
    if tpr != host_tpr {
        core::arch::asm!("mov cr8, {}", in(reg) host_tpr, options(nomem, nostack, preserves_flags));
    }
    status
}

global_asm!(
    ".section .text, \"ax\"",
    ".global svm_world_switch",
    "svm_world_switch:",
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
    ".global vmx_world_switch",
    "vmx_world_switch:",
    ".byte 0xF3, 0x0F, 0x1E, 0xFA", // ENDBR64
    "push rbp",
    "push rbx",
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    "push rdi", // [rsp] = gpr_ptr
    // Update HOST_RSP (0x6C14) to current rsp
    "mov rax, 0x6C14",
    "vmwrite rax, rsp",
    // Update HOST_RIP (0x6C16) to VM-exit landing pad (label 2)
    "lea rdx, [rip + 2f]",
    "mov rax, 0x6C16",
    "vmwrite rax, rdx",
    // Test launched flag (rsi)
    "test rsi, rsi",
    "jnz 10f",
    // vmlaunch path (first launch)
    "mov rax, [rdi + 0]",
    "mov rcx, [rdi + 8]",
    "mov rdx, [rdi + 16]",
    "mov rbx, [rdi + 24]",
    "mov rbp, [rdi + 40]",
    "mov rsi, [rdi + 48]",
    "mov r8,  [rdi + 64]",
    "mov r9,  [rdi + 72]",
    "mov r10, [rdi + 80]",
    "mov r11, [rdi + 88]",
    "mov r12, [rdi + 96]",
    "mov r13, [rdi + 104]",
    "mov r14, [rdi + 112]",
    "mov r15, [rdi + 120]",
    "mov rdi, [rdi + 56]",
    "vmlaunch",
    "jmp 3f",
    // vmresume path (subsequent resumes)
    "10:",
    "mov rax, [rdi + 0]",
    "mov rcx, [rdi + 8]",
    "mov rdx, [rdi + 16]",
    "mov rbx, [rdi + 24]",
    "mov rbp, [rdi + 40]",
    "mov rsi, [rdi + 48]",
    "mov r8,  [rdi + 64]",
    "mov r9,  [rdi + 72]",
    "mov r10, [rdi + 80]",
    "mov r11, [rdi + 88]",
    "mov r12, [rdi + 96]",
    "mov r13, [rdi + 104]",
    "mov r14, [rdi + 112]",
    "mov r15, [rdi + 120]",
    "mov rdi, [rdi + 56]",
    "vmresume",
    "jmp 3f",
    // VM-entry failed: CPU sets CF (VMfailInvalid) or ZF (VMfailValid)
    "3:",
    "pop rdi",
    "jc 4f",
    "jz 5f",
    "mov eax, 3",
    "jmp 6f",
    "4:",
    "mov eax, 1",
    "jmp 6f",
    "5:",
    "mov eax, 2",
    "jmp 6f",
    "6:",
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop rbx",
    "pop rbp",
    "ret",
    // VM-exit landing pad (CPU jumped here on exit, with RSP=HOST_RSP)
    "2:",
    "push rax",
    "mov rax, [rsp + 8]", // gpr_ptr
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
    "pop qword ptr [rax + 0]", // guest RAX
    "pop rdi",                 // pop gpr_ptr
    "xor eax, eax",            // return 0 (successful exit)
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop rbx",
    "pop rbp",
    "ret",
);
