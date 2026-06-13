//! x86_64 exception / syscall register save frame.
//!
//! Layout mirrors the RISC-V ViTrapFrame shape (32 regs + sstatus + sepc +
//! stval + scause) so kernel/src/task*.rs compiles across architectures.
//!
//! ## Authoritative register-to-index map
//!
//! All assembly (`__trap_exit`, syscall entry, interrupt stubs) MUST use
//! these offsets exclusively.  The byte offset for `regs[N]` is `N * 8`.
//!
//! | Index | Offset | x86_64 register | Notes                              |
//! |-------|--------|-----------------|------------------------------------|
//! | 0     |   +0   | (padding/unused)| mirrors x0==0 slot on RISC-V       |
//! | 1     |   +8   | RCX             | user RIP on SYSCALL; callee-saved   |
//! | 2     |  +16   | RSP             | user stack pointer                  |
//! | 3     |  +24   | RBX             | callee-saved                       |
//! | 4     |  +32   | RBP             | callee-saved / frame pointer        |
//! | 5     |  +40   | RSI             | syscall arg1                        |
//! | 6     |  +48   | RDI             | syscall arg0                        |
//! | 7     |  +56   | R8              | syscall arg4                        |
//! | 8     |  +64   | R9              | syscall arg5                        |
//! | 9     |  +72   | R10             | syscall arg3 (SYSCALL ABI)          |
//! | 10    |  +80   | R11             | user RFLAGS on SYSCALL; clobbered   |
//! | 11    |  +88   | R12             | callee-saved                       |
//! | 12    |  +96   | R13             | callee-saved                       |
//! | 13    | +104   | R14             | callee-saved                       |
//! | 14    | +112   | R15             | callee-saved                       |
//! | 15    | +120   | RDX             | syscall arg2                        |
//! | 16    | +128   | (unused/zero)   |                                    |
//! | 17    | +136   | RAX             | syscall number / return value       |
//! | 18–26 | +144…  | (unused/zero)   | scratch area reused by __trap_exit  |
//! | 27    | +216   | (iretq: RIP)    | __trap_exit scratch: user RIP       |
//! | 28    | +224   | (iretq: CS)     | __trap_exit scratch: 0x23           |
//! | 29    | +232   | (iretq: RFLAGS) | __trap_exit scratch: masked sstatus |
//! | 30    | +240   | (iretq: RSP)    | __trap_exit scratch: user RSP       |
//! | 31    | +248   | (iretq: SS)     | __trap_exit scratch: 0x1B           |
//!
//! Fixed-field offsets (after regs[]):
//! | Field   | Offset | Meaning                                 |
//! |---------|--------|-----------------------------------------|
//! | sstatus | +256   | RFLAGS — 0x202 (IF=1, bit1=1) for new  |
//! | sepc    | +264   | User RIP — entry for new / ret for sys  |
//! | stval   | +272   | CR2 on #PF, 0 otherwise                 |
//! | scause  | +280   | Vector number                           |
//!
//! Total size: 32×8 + 4×8 = **288 bytes**.

#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct ViTrapFrame {
    /// General-purpose register slots (matches RISC-V count for kernel compat).
    /// Populated by the x86_64 syscall / interrupt entry asm (Phase 05).
    /// See module doc for the authoritative index→register mapping.
    pub regs: [usize; 32],
    /// RFLAGS — initialised to 0x202 (IF=1, reserved=1) for new user tasks.
    pub sstatus: usize,
    /// User RIP — entry point for new tasks; return address for syscalls.
    pub sepc: usize,
    /// CR2 on #PF, 0 otherwise.
    pub stval: usize,
    /// Interrupt / exception vector number.
    pub scause: usize,
}

// Compile-time guard: ViTrapFrame must be exactly 288 bytes.
// 32 regs × 8 bytes + 4 fields × 8 bytes = 256 + 32 = 288.
const _: () = assert!(
    core::mem::size_of::<ViTrapFrame>() == 288,
    "ViTrapFrame size mismatch — update the index map in trap.rs"
);

/// Returns `(0, 0)` — x86_64 has no RISC-V GP/TP registers.
#[inline(always)]
pub fn get_gp_tp() -> (usize, usize) {
    (0, 0)
}
