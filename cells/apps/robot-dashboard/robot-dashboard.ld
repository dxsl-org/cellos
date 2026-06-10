OUTPUT_ARCH(riscv)
ENTRY(_start)

SECTIONS
{
    /* robot-dashboard user-space base address: 0x22000000 (544 MB virtual).
     * Avoids: robot-demo (0x20000000-0x20900000) and all other live cells.
     * robot-dashboard has an 8 MiB heap BSS; total footprint ≈ 9 MB → ends
     * at ~0x22900000, well below RAM at 0x80000000. */
    . = 0x22000000;

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
        *(.data.rel.ro .data.rel.ro.*)
        *(.got .got.* .got.plt)
        *(.sdata .sdata.*)
    }

    .bss : ALIGN(4096) {
        __bss_start = .;
        *(.bss .bss.*)
        *(.sbss .sbss.*)
        *(COMMON)
        __bss_end = .;
    }

    __ViCell_manifest : ALIGN(4096) {
        KEEP(*(__ViCell_manifest))
    }
}
