OUTPUT_ARCH(aarch64)
ENTRY(_start)

SECTIONS
{
    /* Compositor service — AArch64 ARM virt.
     * 0x1A000000 (416 MB): above VirtIO (0x0A00_4000) and GIC (0x0900_0000).
     * Free on RISC-V (above VirtIO probe at 0x1001_0000, below RAM at 0x8000_0000). */
    . = 0x1A000000;

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

    __ViCell_manifest : ALIGN(4096) {
        KEEP(*(__ViCell_manifest))
    }
}
