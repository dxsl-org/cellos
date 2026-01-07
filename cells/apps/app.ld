OUTPUT_ARCH(riscv)
ENTRY(_start)

SECTIONS
{
    /* App base address - Offset from Kernel */
    . = 0x88000000;
    
    .text : {
        *(.text.boot)
        *(.text .text.*)
    }
    
    .rodata : ALIGN(4096) {
        *(.rodata .rodata.*)
    }
    
    .data : ALIGN(4096) {
        PROVIDE(__global_pointer$ = . + 0x800);
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
}
