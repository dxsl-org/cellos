OUTPUT_ARCH(i386:x86-64)
ENTRY(_start)

SECTIONS
{
    /* Lua runtime cell — same VA window as other arches.
     * No __global_pointer$ (RISC-V only). */
    . = 0x1C000000;

    .text : {
        *(.text.boot)
        *(.text .text.*)
    }

    .rodata : ALIGN(4096) {
        *(.rodata .rodata.*)
    }

    .data : ALIGN(4096) {
        *(.data .data.*)
        *(.data.rel.ro .data.rel.ro.*)
        *(.got .got.* .got.plt)
    }

    .bss : ALIGN(4096) {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        __bss_end = .;
    }

    . = ALIGN(4096);
}
