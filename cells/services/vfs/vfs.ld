OUTPUT_ARCH(riscv)
ENTRY(_start)

SECTIONS
{
    /* VFS user-space base address — RISC-V SV39 user VA (< 0x8000_0000).
     * 32 MB offset leaves room for init/config below and other services above. */
    . = 0x02000000;
    
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

    .vicell_manifest : ALIGN(8) {
        KEEP(*(__ViCell_manifest))
    }
}
