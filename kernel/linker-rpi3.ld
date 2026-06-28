OUTPUT_ARCH(aarch64)
ENTRY(_start)

SECTIONS {
    /*
     * VideoCore firmware loads kernel8.img at physical 0x80000.
     * RAM starts at 0x00000000; GPU reserves the top 64 MiB (0x3C000000+).
     * Stack grows downward from below the load address — safe because
     * VideoCore uses only the first 0x1000 bytes for its own vectors.
     */
    . = 0x80000;

    .text.boot : {
        KEEP(*(.text.boot))
    }

    .text.vectors : ALIGN(2048) {
        KEEP(*(.text.vectors))
    }

    .text : ALIGN(4096) {
        *(.text .text.*)
    }

    .rodata : ALIGN(4096) {
        *(.rodata .rodata.*)
    }

    .data : ALIGN(4096) {
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

    /* 64 KiB kernel stack placed after the kernel image. */
    .bss.stack : ALIGN(4096) {
        . = . + 0x10000;
        __stack_top = .;
    }
}
