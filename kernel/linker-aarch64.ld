OUTPUT_ARCH(aarch64)
ENTRY(_start)

SECTIONS {
    /* QEMU virt loads kernel at 0x40080000 by default. */
    . = 0x40080000;

    .text.boot : {
        __domain_text_start = .;
        KEEP(*(.text.boot))
    }

    .text.vectors : ALIGN(2048) {
        KEEP(*(.text.vectors))
    }

    .text : ALIGN(4096) {
        *(.text .text.*)
        . = ALIGN(4096);
        __domain_text_end = .;
    }

    .rodata : ALIGN(4096) {
        __domain_readonly_start = .;
        *(.rodata .rodata.*)
        *(.requests_start_marker)
        *(.requests_end_marker)
        *(.eh_frame_hdr)
        *(.eh_frame)
        . = ALIGN(4096);
        __domain_readonly_end = .;
    }

    .data : ALIGN(4096) {
        __domain_writable_start = .;
        *(.got .got.*)
        *(.requests)
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

    /* 64KB kernel stack */
    .bss.stack : ALIGN(4096) {
        . = . + 0x10000;
        __stack_top = .;
        . = ALIGN(4096);
        __domain_writable_end = .;
    }

    /* Linux arm64 Image header fields (boot.rs `_start`, header-carrying
     * backends only). text_offset is the image's offset above a 2 MiB-aligned
     * load base; image_size spans the load address to the end of the boot
     * stack, which the loader must keep free. */
    __image_text_offset = __domain_text_start & 0x1FFFFF;
    __image_size = __domain_writable_end - __domain_text_start;
}
