OUTPUT_ARCH(riscv)
ENTRY(_start)

SECTIONS
{
    /* Lua runtime cell.
     * VA 0x0C000000 (192 MB) — between compositor (0x0A000000) and
     * micropython (0x0E000000). Without an explicit script the linker placed
     * Lua at 0x10000, a low address that overlaps other mappings and prevented
     * the cell from running when spawned. */
    . = 0x0C000000;

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
