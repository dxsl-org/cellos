//! x86_64 SYSCALL/SYSRET MSR configuration and dispatch.
//!
//! ## Syscall ABI translation
//!
//! x86_64 syscall ABI: RAX=number, args in RDI/RSI/RDX/R10/R8/R9.
//! Hardware saves: RCX=user RIP, R11=user RFLAGS, then clears IF (via FMASK).
//!
//! The kernel dispatcher `ViCell_syscall_dispatch` uses the RISC-V index
//! convention (regs[17]=number, regs[10]=a0, …).  `syscall_entry` translates:
//!
//! | x86_64 reg | ViTrapFrame slot | Offset | Role                  |
//! |------------|-----------------|--------|-----------------------|
//! | RAX        | regs[17]        | +136   | syscall number        |
//! | RDI        | regs[10]        | +80    | arg0 / return value   |
//! | RSI        | regs[11]        | +88    | arg1                  |
//! | RDX        | regs[12]        | +96    | arg2                  |
//! | R10        | regs[13]        | +104   | arg3 (SYSCALL ABI)    |
//! | R8         | regs[14]        | +112   | arg4                  |
//! | R9         | regs[15]        | +120   | arg5                  |
//! | RSP (user) | regs[2]         | +16    | user stack pointer    |
//! | RCX        | sepc            | +264   | return RIP            |
//! | R11        | sstatus         | +256   | saved RFLAGS          |
//! | RBX        | regs[3]         | +24    | callee-saved          |
//! | RBP        | regs[4]         | +32    | callee-saved          |
//! | R12        | regs[18]        | +144   | callee-saved (scratch)|
//! | R13        | regs[19]        | +152   | callee-saved (scratch)|
//! | R14        | regs[20]        | +160   | callee-saved (scratch)|
//! | R15        | regs[21]        | +168   | callee-saved (scratch)|
//!
//! Note: regs[10..15] hold syscall args in this path, NOT the x86-specific
//! register assignment from the exception-path doc in trap.rs (which is only
//! authoritative for `__trap_exit` context restoration, not for SYSCALL entry).
use super::trap::ViTrapFrame;
use core::arch::asm;
use core::arch::global_asm;

const IA32_EFER: u32 = 0xC000_0080;
const IA32_STAR: u32 = 0xC000_0081;
const IA32_LSTAR: u32 = 0xC000_0082;
const IA32_FMASK: u32 = 0xC000_0084;
const IA32_KERNEL_GSBASE: u32 = 0xC000_0102; // Swapped into GS_BASE by swapgs

/// Per-CPU storage used by the `swapgs`-based stack swap in syscall_entry.
///
/// Layout (offsets are load-bearing — referenced directly in asm):
///   gs:0  = kernel RSP (loaded on syscall entry)
///   gs:8  = scratch (user RSP saved here during syscall)
///   gs:16 = PKU value for the current task (loaded into PKRU on ring-3 re-entry)
///
/// KERNEL_GS_BASE MSR must point here before any Ring-3 entry.
/// `set_cpu_local` initialises this; `set_kernel_stack` updates slot [0];
/// `set_task_pku` updates slot [16].
#[repr(C, align(16))]
struct CpuLocal {
    kernel_rsp: u64, // offset 0  — gs:0
    user_rsp: u64,   // offset 8  — gs:8
    pku_value: u32,  // offset 16 — gs:16 (restored to PKRU on ring-3 re-entry)
    _pad: u32,       // offset 20 — alignment padding
}
static mut CPU_LOCAL: CpuLocal = CpuLocal {
    kernel_rsp: 0,
    user_rsp: 0,
    pku_value: 0,
    _pad: 0,
};

fn rdmsr(msr: u32) -> u64 {
    let lo: u32;
    let hi: u32;
    // SAFETY: rdmsr from Ring 0 does not affect memory safety.
    unsafe {
        asm!("rdmsr", in("ecx") msr, out("eax") lo, out("edx") hi, options(nomem,nostack));
    }
    (hi as u64) << 32 | lo as u64
}
fn wrmsr(msr: u32, val: u64) {
    let lo = val as u32;
    let hi = (val >> 32) as u32;
    // SAFETY: wrmsr to a valid MSR from Ring 0 does not affect memory safety.
    unsafe {
        asm!("wrmsr", in("ecx") msr, in("eax") lo, in("edx") hi, options(nomem,nostack));
    }
}

/// Initialise SYSCALL/SYSRET path and per-CPU GS area.
///
/// Must be called from Ring 0 before any Ring-3 entry.  Sets up:
/// - EFER.SCE so the CPU honours the SYSCALL instruction
/// - STAR/LSTAR/FMASK for the entry point and segment selectors
/// - KERNEL_GS_BASE pointing at `CPU_LOCAL` so `swapgs` in the syscall
///   entry stub can load the kernel stack without touching user memory
pub fn init() {
    wrmsr(IA32_EFER, rdmsr(IA32_EFER) | 1); // SCE=1
                                            // STAR[47:32] = kernel CS (syscall: CS=0x08, SS=0x10).
                                            // STAR[63:48] = user_CS_base for sysretq: CS = (base+16)|3, SS = (base+8)|3.
                                            // Our GDT: uCS=0x20 (GDT[4]), uDS=0x18 (GDT[3]).
                                            // To get sysretq CS=0x23, SS=0x1B: base = 0x20-16 = 0x10.
                                            // NOTE: using 0x0020 here gives CS=0x0033 (TSS high!), breaking iretq from ISR.
    wrmsr(IA32_STAR, (0x0010_u64 << 48) | (0x0008_u64 << 32));
    extern "C" {
        fn syscall_entry();
    }
    wrmsr(IA32_LSTAR, syscall_entry as *const () as u64);
    wrmsr(IA32_FMASK, 0x0300); // clear IF + DF on syscall entry

    // Point KERNEL_GS_BASE at the per-CPU area so swapgs in syscall_entry
    // exchanges GS_BASE with KERNEL_GS_BASE and gives us %gs:0 / %gs:8.
    // SAFETY: CPU_LOCAL is a static; addr_of! gives a raw pointer without
    // creating a Rust reference.
    // addr_of! on a static does not require unsafe (no Rust reference created).
    let cpu_local_addr = core::ptr::addr_of!(CPU_LOCAL) as u64;
    wrmsr(IA32_KERNEL_GSBASE, cpu_local_addr);
}

/// Update the kernel-stack pointer stored in the per-CPU area.
///
/// Called by the scheduler before every Ring-3 entry so that `swapgs` +
/// `movq %gs:0, %rsp` in `syscall_entry` loads the correct kernel stack.
///
/// Also reinitialises IA32_KERNEL_GS_BASE to `&CPU_LOCAL` on every call.
/// When Task A enters a syscall, the entry `swapgs` stores Task A's user GS
/// (= 0 for ViCell cells) into KERNEL_GS_BASE and puts &CPU_LOCAL in GS_BASE.
/// If Task A blocks before its exit `swapgs`, KERNEL_GS_BASE remains 0.
/// The next task's syscall entry `swapgs` would then load GS_BASE = 0 and
/// fault at `%gs:8` (va = 0x8).  Resetting KERNEL_GS_BASE here is safe
/// because ViCell cells do not use the GS segment (user GS is always 0).
pub fn set_kernel_stack(sp: u64) {
    // SAFETY: CPU_LOCAL is a static with no aliased Rust references here.
    // wrmsr from Ring 0 is safe; we only touch the GS MSRs.
    unsafe {
        CPU_LOCAL.kernel_rsp = sp;
        let cpu_local_addr = core::ptr::addr_of!(CPU_LOCAL) as u64;
        wrmsr(IA32_KERNEL_GSBASE, cpu_local_addr);
    }
}

/// Update the PKU value for the current task.
///
/// Called by the scheduler alongside `set_kernel_stack` before every ring-3 re-entry.
/// The value is loaded from `gs:16` into PKRU by the asm exit paths in `syscall_entry`
/// (sysretq path) and `__trap_exit` (iretq path).
///
/// # Safety contract
/// CPU_LOCAL is a per-CPU static; this is the only writer (scheduler, single-core SAS).
/// No concurrent Rust reference to `pku_value` exists.
pub fn set_task_pku(val: u32) {
    // SAFETY: CPU_LOCAL is a static; access is single-threaded (single-core SAS).
    // The asm paths read gs:16 — this write is sequenced before the ring-3 entry
    // by the scheduler lock that also surrounds set_kernel_stack.
    unsafe {
        CPU_LOCAL.pku_value = val;
    }
}

extern "Rust" {
    // SAFETY: `ViCell_syscall_dispatch` is defined in kernel/src/task/syscall.rs
    // with `#[no_mangle] pub extern "Rust"`.  It is called only from
    // `syscall_entry` below, which has already built the full ViTrapFrame on
    // the kernel stack and passes a valid `&mut ViTrapFrame` pointer in RDI.
    //
    // allow(dead_code): this declaration is never referenced from Rust — the
    // `global_asm!` stub below calls it directly by symbol name (`call
    // ViCell_syscall_dispatch`), so the linker resolves it even though rustc
    // sees no caller. The `extern "Rust"` block exists solely to assert the
    // ABI/signature contract with kernel/src/task/syscall.rs at compile time.
    #[allow(dead_code)]
    fn ViCell_syscall_dispatch(frame: &mut ViTrapFrame);
}

// The syscall_entry stub is written in AT&T syntax. Rust's global_asm!
// defaults to Intel syntax on every target, so we MUST pass
// options(att_syntax) or the `%reg` / src,dst operand order fails to parse.
//
// On SYSCALL entry (hardware invariants):
//   RCX = user RIP (return address)
//   R11 = user RFLAGS (saved by CPU; IF cleared via FMASK=0x0300)
//   RSP = still user RSP
//   CS  = kernel CS from STAR[47:32]
//
// Frame layout on kernel stack (288 bytes total, 16-byte aligned):
//   RSP+0   … RSP+255  = regs[0..31]   (256 bytes, 32 × 8)
//   RSP+256 = sstatus  (saved RFLAGS = R11)
//   RSP+264 = sepc     (return RIP = RCX)
//   RSP+272 = stval    (0, unused in syscall path)
//   RSP+280 = scause   (0, unused in syscall path)
//
// Argument mapping into ViTrapFrame (RISC-V a-register convention):
//   regs[10]  +80  ← RDI  (arg0 / return-value slot)
//   regs[11]  +88  ← RSI  (arg1)
//   regs[12]  +96  ← RDX  (arg2)
//   regs[13] +104  ← R10  (arg3)
//   regs[14] +112  ← R8   (arg4)
//   regs[15] +120  ← R9   (arg5)
//   regs[17] +136  ← RAX  (syscall number)
//   regs[2]  +16   ← user RSP (from %gs:8)
//   regs[3]  +24   ← RBX  (callee-saved: preserve across dispatch)
//   regs[4]  +32   ← RBP  (callee-saved: preserve across dispatch)
//   regs[6]  +48   ← RDI  (second copy: regs[10] is overwritten by the return
//                          value, exit restores original RDI from here)
//   regs[18] +144  ← R12  (callee-saved: scratch slots regs[18..21])
//   regs[19] +152  ← R13
//   regs[20] +160  ← R14
//   regs[21] +168  ← R15
//   sstatus  +256  ← R11  (RFLAGS)
//   sepc     +264  ← RCX  (return RIP)
//   stval    +272  = 0
//   scause   +280  = 0
//
// After dispatch: RAX ← frame.regs[10] (return value); RCX/R11 ← sepc/sstatus;
// RDI/RSI/RDX/R10/R8/R9 ← original argument values (preserve-all contract —
// user code may cache live values, even the next syscall number, in any of
// them; only RAX/RCX/R11 are clobbered, matching the ostd stub's asm! decl).
global_asm!(
    r#"
    .section .text
    .global syscall_entry
    .balign 16
syscall_entry:
    # CET-IBT landing pad: SYSCALL is a hardware-directed branch so technically
    # ENDBR64 is not required by the CPU here, but having it keeps this address
    # valid for any static-analysis tool or future IBT audit of the syscall path.
    # Encoding: F3 0F 1E FA (NOP on non-CET CPUs).
    .byte 0xF3, 0x0F, 0x1E, 0xFA
    swapgs
    # PKU: enter kernel domain (PKRU=0 = all-access).
    # Guard: wrpkru is #UD on CPUs without PKU — check ViCell_pku_active first.
    cmpb $0, ViCell_pku_active(%rip)
    je .Lpku_entry_skip
    xorl %eax, %eax        # PKRU = 0 (all-access for kernel)
    xorl %ecx, %ecx        # WRPKRU precondition: ECX must be 0
    xorl %edx, %edx        # WRPKRU precondition: EDX must be 0
    wrpkru
.Lpku_entry_skip:
    # Save user RSP into per-CPU scratch; load kernel RSP.
    movq %rsp, %gs:8
    movq %gs:0, %rsp

    # Allocate 288-byte ViTrapFrame on the kernel stack.
    # 288 % 16 == 0, so RSP is still 16-byte aligned here.
    subq $288, %rsp

    # --- Zero out the slots we do not explicitly write ---
    # regs[0]  (+0): always 0 (mirrors x0 on RISC-V)
    movq $0,   0(%rsp)
    # regs[1]  (+8):  unused in syscall path
    movq $0,   8(%rsp)
    # regs[5]  (+40):  RSI physical slot — unused (RSI→regs[11] for dispatch)
    movq $0,  40(%rsp)
    # regs[6]  (+48):  original RDI — regs[10] is overwritten by the return
    # value, so the exit path restores RDI from here (preserve-all contract:
    # the ostd stub declares rdi/rsi/rdx/r10 as `in` and never mentions r8/r9,
    # so the compiler assumes every register except rax/rcx/r11 survives).
    movq %rdi, 48(%rsp)
    # regs[7]  (+56):  R8 physical slot — unused (R8→regs[14] for dispatch)
    movq $0,  56(%rsp)
    # regs[8]  (+64):  R9 physical slot — unused (R9→regs[15] for dispatch)
    movq $0,  64(%rsp)
    # regs[9]  (+72):  R10 physical slot — unused (R10→regs[13] for dispatch)
    movq $0,  72(%rsp)
    # regs[16] (+128): unused
    movq $0, 128(%rsp)
    # regs[22..26] (+176..+208): unused scratch
    movq $0, 176(%rsp)
    movq $0, 184(%rsp)
    movq $0, 192(%rsp)
    movq $0, 200(%rsp)
    movq $0, 208(%rsp)
    # regs[27..31] (+216..+248): __trap_exit iretq scratch, 0 for syscall path
    movq $0, 216(%rsp)
    movq $0, 224(%rsp)
    movq $0, 232(%rsp)
    movq $0, 240(%rsp)
    movq $0, 248(%rsp)
    # stval (+272) and scause (+280): 0 for syscall path
    movq $0, 272(%rsp)
    movq $0, 280(%rsp)

    # --- Save syscall arguments into RISC-V a-register slots ---
    movq %rdi,  80(%rsp)    # regs[10] = arg0
    movq %rsi,  88(%rsp)    # regs[11] = arg1
    movq %rdx,  96(%rsp)    # regs[12] = arg2
    movq %r10, 104(%rsp)    # regs[13] = arg3 (R10 not RCX per x86 SYSCALL ABI)
    movq %r8,  112(%rsp)    # regs[14] = arg4
    movq %r9,  120(%rsp)    # regs[15] = arg5
    movq %rax, 136(%rsp)    # regs[17] = syscall number

    # --- Save user RSP (was stashed in %gs:8 above) ---
    movq %gs:8, %rax
    movq %rax,  16(%rsp)    # regs[2] = user RSP

    # --- Save callee-saved registers (needed if scheduler preempts) ---
    movq %rbx,  24(%rsp)    # regs[3]
    movq %rbp,  32(%rsp)    # regs[4]
    movq %r12, 144(%rsp)    # regs[18] (spare area)
    movq %r13, 152(%rsp)    # regs[19]
    movq %r14, 160(%rsp)    # regs[20]
    movq %r15, 168(%rsp)    # regs[21]

    # --- Save SYSCALL-hardwired return state ---
    movq %r11, 256(%rsp)    # sstatus = saved RFLAGS (R11 by hardware)
    movq %rcx, 264(%rsp)    # sepc    = return RIP   (RCX by hardware)

    # Call ViCell_syscall_dispatch(&mut frame).
    # RSP is 16-byte aligned here (288 % 16 == 0); the CALL pushes 8 bytes
    # making RSP 8-byte aligned at the callee entry — correct per SysV ABI.
    movq %rsp, %rdi          # arg0 = *mut ViTrapFrame
    call ViCell_syscall_dispatch

    # --- Restore callee-saved (dispatcher may have rescheduled) ---
    movq 144(%rsp), %r12
    movq 152(%rsp), %r13
    movq 160(%rsp), %r14
    movq 168(%rsp), %r15
    movq  24(%rsp), %rbx
    movq  32(%rsp), %rbp

    # Return value is in frame.regs[10] (+80).
    movq 80(%rsp), %rax

    # PKU: restore user PKRU before ring-3 re-entry.
    # CRITICAL: must run BEFORE loading %rcx with the return RIP below —
    # wrpkru requires xorl %ecx which destroys %rcx.
    cmpb $0, ViCell_pku_active(%rip)
    je .Lpku_exit_skip
    movl %gs:16, %eax          # pku_value from CPU_LOCAL (offset 16)
    xorl %ecx, %ecx            # WRPKRU precondition: ECX must be 0
    xorl %edx, %edx            # WRPKRU precondition: EDX must be 0
    wrpkru                     # PKRU := EAX
    movq 80(%rsp), %rax        # reload return value (eax was overwritten by pku_value)
.Lpku_exit_skip:
    # Reload RCX (return RIP) and R11 (RFLAGS) for SYSRET.
    movq 264(%rsp), %rcx     # sepc    → RCX
    movq 256(%rsp), %r11     # sstatus → R11

    # CVE-2012-0217: Intel #GP if SYSRET executes with non-canonical RCX.
    # Check BEFORE restoring %rdi below so it can serve as scratch.
    movq  %rcx, %rdi
    sarq  $47,  %rdi         # canonical user → 0; kernel/non-canonical → non-zero
    jnz   1f                 # non-canonical: trap, do not sysretq

    # Restore caller-saved argument registers (preserve-all contract).
    # The ostd syscall stub declares rdi/rsi/rdx/r10 as `in` (NOT clobbered)
    # and never mentions r8/r9 — the compiler is entitled to keep live values
    # (including a cached syscall number) in ANY of them across `syscall`.
    # Leaking kernel garbage here corrupted the NEXT syscall's RAX (P02 bug:
    # println's second write arrived as SetTimer).
    movq  88(%rsp), %rsi     # regs[11] = original arg1
    movq  96(%rsp), %rdx     # regs[12] = original arg2
    movq 104(%rsp), %r10     # regs[13] = original arg3
    movq 112(%rsp), %r8      # regs[14] = original arg4
    movq 120(%rsp), %r9      # regs[15] = original arg5
    movq  48(%rsp), %rdi     # regs[6]  = original arg0 (saved at entry)

    # Close the IRQ window for the final descent. If this syscall blocked,
    # yield_cpu's resume path ran `sti`, so IF=1 here — an IRQ landing after
    # the RSP switch would push its frame onto the USER stack at CPL0, and
    # after swapgs the handler would run with GS_BASE = user GS. Both are
    # survivable in SAS but fragile; mask until sysretq restores IF from R11.
    cli

    # Switch to user RSP saved at entry (frame.regs[2], +16). Reading %gs:8
    # instead would be wrong when this task blocked via yield_cpu(): another
    # task's syscall overwrites CPU_LOCAL.user_rsp. This is the LAST frame
    # access — the kernel frame is unreachable after this instruction.
    movq  16(%rsp), %rsp
    swapgs
    sysretq
1:  # Non-canonical RCX: trap — caller has a bug or is malicious.
    ud2
"#,
    options(att_syntax)
);
