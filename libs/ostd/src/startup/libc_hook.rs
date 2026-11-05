//! Weak default for `__cellos_libc_start(entry_sp)` (CP1
//! cell-initial-stack-auxv, AX-6).
//!
//! `_start` calls this hook once, before `main`, with the entry stack pointer
//! (the Cell initial stack: argc, argv, envp, auxv). This default ignores the
//! argument and runs `.init_array` in order — the behaviour every Cell had
//! before the hook existed. A C library that must initialise before any
//! constructor (mlibc: TLS, Tcb, environ) defines the symbol strongly; it then
//! owns running `.init_array` exactly once, and this default is not linked.

#[cfg(target_arch = "riscv64")]
core::arch::global_asm!(
    ".section .text.__cellos_libc_start,\"ax\",@progbits",
    ".weak __cellos_libc_start",
    ".type __cellos_libc_start,@function",
    "__cellos_libc_start:",
    "addi sp, sp, -32",
    "sd   ra, 24(sp)",
    "sd   s0, 16(sp)",
    "sd   s1, 8(sp)",
    // PC-relative, like the old `_start` loop: valid before any GOT use.
    "lla  s0, __init_array_start",
    "lla  s1, __init_array_end",
    "1:",
    "beq  s0, s1, 2f",
    "ld   t0, 0(s0)",
    "jalr t0",
    "addi s0, s0, 8",
    "j    1b",
    "2:",
    "ld   ra, 24(sp)",
    "ld   s0, 16(sp)",
    "ld   s1, 8(sp)",
    "addi sp, sp, 32",
    "ret",
    ".size __cellos_libc_start, . - __cellos_libc_start",
);

#[cfg(target_arch = "aarch64")]
core::arch::global_asm!(
    ".section .text.__cellos_libc_start,\"ax\",%progbits",
    ".weak __cellos_libc_start",
    ".type __cellos_libc_start,%function",
    "__cellos_libc_start:",
    "hint #34", // BTI c (harmless when entered by a direct BL)
    "stp  x29, x30, [sp, #-32]!",
    "stp  x19, x20, [sp, #16]",
    "adrp x19, __init_array_start",
    "add  x19, x19, :lo12:__init_array_start",
    "adrp x20, __init_array_end",
    "add  x20, x20, :lo12:__init_array_end",
    "1:",
    "cmp  x19, x20",
    "b.eq 2f",
    "ldr  x9, [x19], #8",
    "blr  x9",
    "b    1b",
    "2:",
    "ldp  x19, x20, [sp, #16]",
    "ldp  x29, x30, [sp], #32",
    "ret",
    ".size __cellos_libc_start, . - __cellos_libc_start",
);

#[cfg(target_arch = "x86_64")]
core::arch::global_asm!(
    ".section .text.__cellos_libc_start,\"ax\",@progbits",
    ".weak __cellos_libc_start",
    ".type __cellos_libc_start,@function",
    "__cellos_libc_start:",
    "endbr64",
    // Entry rsp ≡ 8 (mod 16); two pushes and 8 bytes keep calls aligned.
    "push rbx",
    "push r12",
    "sub  rsp, 8",
    "lea  rbx, [rip + __init_array_start]",
    "lea  r12, [rip + __init_array_end]",
    "1:",
    "cmp  rbx, r12",
    "je   2f",
    "call [rbx]",
    "add  rbx, 8",
    "jmp  1b",
    "2:",
    "add  rsp, 8",
    "pop  r12",
    "pop  rbx",
    "ret",
    ".size __cellos_libc_start, . - __cellos_libc_start",
);
