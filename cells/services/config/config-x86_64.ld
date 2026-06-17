OUTPUT_ARCH(i386:x86-64)
ENTRY(_start)

SECTIONS
{
    /* Config service user-space base — x86_64.
     * 0x0080_0000 (8 MB): matches config.ld (RISC-V) VA to keep the address map
     * consistent across architectures.  Above init (0x0040_0000 + ~23 KB heap gap)
     * and well below shell (0x0800_0000). */
    . = 0x00800000;

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

    /* Cell manifest: kernel reads this section to validate spawn-time caps. */
    __ViCell_manifest : ALIGN(4096) {
        KEEP(*(__ViCell_manifest))
    }

    /* Syscall allowlist emitted by api::declare_syscalls!(). */
    __ViCell_syscalls : ALIGN(4096) {
        KEEP(*(__ViCell_syscalls))
    }
}
