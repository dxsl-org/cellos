OUTPUT_ARCH(riscv)
ENTRY(_start)

SECTIONS
{
    /* Hypervisor service — RISC-V stub (hypervisor requires ARM64 EL2; RISC-V cell
     * compiles but exits immediately with NotSupported at runtime). */
    . = 0x1C000000;

    .text : {
        *(.text.boot)
        *(.text .text.*)
    }

    .rodata : ALIGN(4096) {
        *(.rodata .rodata.*)
    }

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
