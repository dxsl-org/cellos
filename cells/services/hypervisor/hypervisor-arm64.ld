OUTPUT_ARCH(aarch64)
ENTRY(_start)

SECTIONS
{
    /* Hypervisor service — AArch64.
     * 0x1C000000 (448 MB): above compositor (0x1A000000), below periph-demo (0x22000000).
     * Needs ~4 MB budget for Alpine image buffers in BSS. */
    . = 0x1C000000;

    .text : {
        *(.text.boot)
        *(.text .text.*)
    }

    .rodata : ALIGN(4096) {
        *(.rodata .rodata.*)
    }

    /* ViCell manifest sections — must be page-aligned off .bss (Law: linker gotcha). */
    .vicell_manifest : ALIGN(4096) {
        KEEP(*(.vicell_manifest))
    }
    __ViCell_manifest = ADDR(.vicell_manifest);

    .vicell_syscalls : ALIGN(8) {
        KEEP(*(.vicell_syscalls))
    }
    __ViCell_syscalls = ADDR(.vicell_syscalls);

    .data : ALIGN(4096) {
        *(.data .data.*)
    }

    .bss : ALIGN(4096) {
        *(.bss .bss.*)
        *(COMMON)
    }

    /DISCARD/ : {
        *(.comment)
        *(.eh_frame)
        *(.note.*)
    }
}
