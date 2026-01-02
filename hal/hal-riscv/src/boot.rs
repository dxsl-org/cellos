/// Boot module - Assembly entry point and early initialization
/// 
/// This module handles the transition from bootloader to Rust code

use core::arch::global_asm;

// Assembly boot code
global_asm!(
    r#"
    .section .text.boot
    .global _start
_start:
    # Disable interrupts
    csrw sie, zero
    
    # Set up stack pointer
    la sp, __stack_end
    
    # Clear BSS section
    la t0, __bss_start
    la t1, __bss_end
1:
    bgeu t0, t1, 2f
    sd zero, 0(t0)
    addi t0, t0, 8
    j 1b
2:
    # Jump to Rust entry point (defined in kernel)
    call kmain
    
    # If kmain returns, halt
3:
    wfi
    j 3b
    "#
);
