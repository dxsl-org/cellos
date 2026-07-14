//! RV32 S-mode entry point for QEMU virt + OpenSBI.
//!
//! Linked as a static (non-PIE) ELF — no self-relocation loop needed.
//! OpenSBI drops into S-mode and jumps here with:
//!   a0 = hart ID, a1 = DTB physical address
//! Both values are passed unchanged to `kmain`.

use core::arch::global_asm;

global_asm!(
    r#"
    .section .text.boot
    .global _start
_start:
    # Disable S-mode interrupts before any stack use.
    csrw sie, zero
    csrw sip, zero

    # Initialize global pointer (GP-relaxation requires norelax around lla).
    .option push
    .option norelax
    lla gp, __global_pointer$
    .option pop

    # Clear thread pointer.
    mv tp, zero

    # Set up the kernel stack (top of .kernel_stack section).
    lla sp, __stack_top

    # Clear BSS (4-byte stride for RV32).
    lla t0, __bss_start
    lla t1, __bss_end
1:
    bgeu t0, t1, 2f
    sw   zero, 0(t0)
    addi t0, t0, 4
    j    1b
2:
    # Jump to Rust entry — a0/a1 (hartid/dtb) preserved.
    call kmain

    # kmain is diverging (!); halt if it ever returns.
3:
    wfi
    j 3b
"#
);
