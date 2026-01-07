OUTPUT_ARCH(riscv)
ENTRY(_start)

SECTIONS
{
    /* OpenSBI loads kernel at 0x80200000 */
    . = 0x80200000;
    
    .text : {
        KEEP(*(.text.boot))
        *(.text .text.*)
    }
    
    .rodata : ALIGN(4096) {
        *(.rodata .rodata.*)
    }
    
    .data : ALIGN(4096) {
        . = ALIGN(16);
        __global_pointer$ = . + 0x800;
        *(.data .data.*)
        *(.sdata .sdata.*)
    }
    
    .bss : ALIGN(4096) {
        __bss_start = .;
        *(.bss .bss.*)
        *(.sbss .sbss.*)
        *(COMMON)
        __bss_end = .;
    }
    
    /* Stack (64KB) */
    .stack (NOLOAD) : ALIGN(4096) {
        . = . + 0x10000;
        __stack_top = .;
    }
    
    /DISCARD/ : {
        *(.eh_frame)
    }
}
