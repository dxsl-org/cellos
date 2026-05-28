OUTPUT_ARCH(riscv)
ENTRY(_start)

SECTIONS
{
    /* VFS base address - 0x88500000 (Safe gap after Init 4MB heap) */
    . = 0x88500000;
    
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
