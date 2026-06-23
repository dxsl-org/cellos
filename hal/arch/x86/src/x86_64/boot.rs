//! x86_64 boot entry: _start → kmain_x86 → kmain.
//!
//! Limine jumps to `_start` with long mode active, paging set up, IF=0,
//! and RSP pointing at a bootloader-reclaimable stack (8-byte aligned).

use core::arch::global_asm;

extern "C" {
    /// Linker symbol: top of the kernel `.stack` section (64 KiB, BSS).
    static __stack_top: u8;
}

global_asm!(
    ".section .text.boot, \"ax\"",
    ".global _start",
    "_start:",
    // Switch to the kernel's own stack before any Rust activation record.
    // This vacates the Limine-reclaimable bootloader stack from the very first
    // instruction.
    "lea rsp, [rip + {stack_top}]",
    // 16-byte align required by the SysV ABI before the first CALL/RET.
    "and rsp, -16",
    // Clear frame pointer for clean backtraces.
    "xor rbp, rbp",
    // Jump (not CALL) so kmain_x86 is the ABI bottom-frame.
    "jmp {entry}",
    stack_top = sym __stack_top,
    entry    = sym kmain_x86,
);

/// Rust bridge from `_start` to `kmain`.
///
/// # Safety
/// Called from `_start` with a valid 16-byte-aligned kernel stack.
/// Passes `hartid=0, dtb=0`; both are ignored on x86_64 inside `kmain`.
#[no_mangle]
pub extern "C" fn kmain_x86() -> ! {
    extern "C" {
        fn kmain(hartid: usize, dtb: usize) -> !;
    }
    // SAFETY: kmain guards all RISC-V-specific accesses behind
    // #[cfg(target_arch = "riscv64")]; hartid/dtb are unused on x86_64.
    unsafe { kmain(0, 0) }
}

// SAFETY: __trap_exit is jumped to (not called) by CpuContext::switch when a
// freshly-spawned user task runs for the first time.  Preconditions:
//   • RSP = tf_ptr, i.e. RSP points at a valid ViTrapFrame (288 bytes, #[repr(C)]).
//   • Interrupts are disabled (IF=0) — the scheduler holds the CPU lock.
//   • The frame was seeded by spawn_from_mem: sepc=entry, regs[2]=user_sp,
//     sstatus=0x202 (IF=1, reserved bit 1 = 1), all other regs = 0.
//
// ViTrapFrame offsets (byte):
//   regs[N]  = N*8     (N = 0..31, total 256 bytes)
//   sstatus  = 256     (RFLAGS for the user task)
//   sepc     = 264     (user RIP / entry point)
//   stval    = 272     (CR2 on #PF — unused here)
//   scause   = 280     (vector — unused here)
//
// Strategy: reuse regs[27..31] (bytes 216..255, always unused/zero) as a
// scratch area to build the 5-word iretq frame in-place.  After building,
// restore GP regs from the lower part of the frame, advance RSP to byte 216,
// and execute iretq.  This avoids any extra stack allocation.
//
// iretq pops (from low to high RSP): RIP, CS, RFLAGS, RSP, SS.
//   regs[27] +216 → user RIP      (from sepc  +264)
//   regs[28] +224 → user CS       (0x23 = GDT user-code | RPL3)
//   regs[29] +232 → user RFLAGS   (sstatus +256, masked to safe bits)
//   regs[30] +240 → user RSP      (regs[2] +16)
//   regs[31] +248 → user SS       (0x1B = GDT user-data | RPL3)
//
// RFLAGS masking: keep only bits 11:0 (status/control flags), force IF=1 and
// reserved-always-1 bit 1.  This strips IOPL (bits 12-13), NT (14), RF (16),
// VM (17), AC (18), VIF (19), VIP (20) and any higher privileged bits from
// a potentially-crafted frame.
global_asm!(
    ".section .text, \"ax\"",
    ".global __trap_exit",
    "__trap_exit:",
    // CET-IBT landing pad: every indirect-branch target must begin with ENDBR64.
    // Encoding: F3 0F 1E FA (acts as a NOP on CPUs that do not support CET).
    ".byte 0xF3, 0x0F, 0x1E, 0xFA",
    // ── Build the 5-word iretq frame in regs[27..31] ────────────────────────
    // user RIP ← sepc (+264)
    "mov rax, [rsp + 264]",
    "mov [rsp + 216], rax",
    // user CS = SEL_USER_CODE = 0x23
    "mov qword ptr [rsp + 224], 0x23",
    // user RFLAGS ← sstatus (+256) masked to safe user bits, IF forced on
    "mov rax, [rsp + 256]",
    "and rax, 0x0FFF",          // keep status/control flags (bits 11:0); clears IOPL, NT, VM, AC
    "or  rax, 0x202",           // force IF=1 (bit 9) + reserved-always-1 bit 1
    "mov [rsp + 232], rax",
    // user RSP ← regs[2] (+16)
    "mov rax, [rsp + 16]",
    "mov [rsp + 240], rax",
    // user SS = SEL_USER_DATA = 0x1B  (GDT uDS 0x18 | RPL3)
    "mov qword ptr [rsp + 248], 0x1B",
    // ── Restore GP registers from ViTrapFrame ───────────────────────────────
    // Load callee-saved and caller-saved regs (skip rax/rcx/rdx/r11 for last).
    // rsp itself is NOT restored here — iretq will load user RSP from the frame.
    "mov rbx, [rsp + 24]",      // regs[3]
    "mov rbp, [rsp + 32]",      // regs[4]
    "mov rsi, [rsp + 40]",      // regs[5]
    "mov rdi, [rsp + 48]",      // regs[6]
    "mov r8,  [rsp + 56]",      // regs[7]
    "mov r9,  [rsp + 64]",      // regs[8]
    "mov r10, [rsp + 72]",      // regs[9]
    "mov r12, [rsp + 88]",      // regs[11]
    "mov r13, [rsp + 96]",      // regs[12]
    "mov r14, [rsp + 104]",     // regs[13]
    "mov r15, [rsp + 112]",     // regs[14]
    "mov rdx, [rsp + 120]",     // regs[15]
    "mov rcx, [rsp + 8]",       // regs[1]
    "mov r11, [rsp + 80]",      // regs[10]
    "mov rax, [rsp + 136]",     // regs[17]
    // ── PKU: restore user PKRU before ring-3 re-entry (iretq path) ──────────
    // Guard: wrpkru causes #UD on CPUs without PKU — test ViCell_pku_active first.
    // ECX must be 0 for wrpkru; this clobbers %rax which is then reloaded.
    "cmp byte ptr [rip + ViCell_pku_active], 0",
    "je 1f",
    "mov eax, dword ptr gs:[16]", // pku_value from CPU_LOCAL (offset 16)
    "xor ecx, ecx",              // WRPKRU precondition: ECX = 0
    "xor edx, edx",              // WRPKRU precondition: EDX = 0
    "wrpkru",                    // PKRU := EAX
    "mov rax, [rsp + 136]",      // reload user rax (was overwritten by pku_value)
    "1:",
    // ── Advance RSP to the iretq frame and enter ring-3 ─────────────────────
    "add rsp, 216",
    "iretq",
);

// SAFETY: thread_trampoline is jumped to (not called) by CpuContext::switch
// when a kernel thread spawned via spawn_thread runs for the first time.
// This is a kernel-thread-only trampoline — the CPU stays at CPL 0.
//
// Register contract on entry (set by spawn_thread x86_64 cfg block):
//   RBX = entry function pointer  (fn(usize) kernel thread body)
//   R12 = argument (usize)
//
// The cooperative switch restores all CpuContext callee-saved fields
// (r15, r14, r13, r12, rbx, rbp) before jumping here, so rbx and r12
// carry the values stored in the task's CpuContext by spawn_thread.
global_asm!(
    ".section .text, \"ax\"",
    ".global thread_trampoline",
    "thread_trampoline:",
    // CET-IBT landing pad: every indirect-branch target must begin with ENDBR64.
    // Encoding: F3 0F 1E FA (acts as a NOP on CPUs that do not support CET).
    ".byte 0xF3, 0x0F, 0x1E, 0xFA",
    // Move arg into first parameter register per SysV AMD64 ABI.
    "mov rdi, r12",
    // Call the kernel thread body.  If it returns, the CPU should not
    // continue — halt with a deliberate fault to surface the bug.
    "call rbx",
    // Thread returned without calling sys_exit — this is a kernel bug.
    // UD2 causes #UD so the fault handler can report the offending TID.
    "ud2",
);
