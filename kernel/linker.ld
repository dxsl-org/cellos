OUTPUT_ARCH(riscv)
ENTRY(_start)

MEMORY
{
    /* 128MB RAM starting at 0x80200000 (OpenSBI jump address) */
    ram (wxa) : ORIGIN = 0x80200000, LENGTH = 128M
}

SECTIONS
{
    /* Kernel code and data
     * KEEP(.text.boot) is mandatory: the bootloader (Limine / OpenSBI jump)
     * enters at _start which lives in .text.boot. Without KEEP, --gc-sections
     * may drop it under release LTO and the kernel never boots.
     */
    .text : {
        KEEP(*(.text.boot))
        *(.text .text.*)
    } >ram

    .rodata : ALIGN(4096) {
        *(.rodata .rodata.*)
        *(.srodata .srodata.*)
    } >ram

    .data : ALIGN(4096) {
        . = ALIGN(16);
        __global_pointer$ = . + 0x800;
        *(.data .data.*)
        *(.sdata .sdata.*)
    } >ram

    .bss : ALIGN(4096) {
        __bss_start = .;
        *(.bss .bss.*)
        *(.sbss .sbss.*)
        *(COMMON)
        __bss_end = .;
    } >ram

    /* Kernel Stack (64KB) - NOLOAD keeps 64KB of zeros out of the kernel image. */
    .kernel_stack (NOLOAD) : ALIGN(4096) {
        . = ALIGN(4096);
        __stack_bottom = .;
        *(.stack)
        . = . + 0x10000;
        __stack_top = .;
    } >ram

    /DISCARD/ : {
        *(.eh_frame)
    }
}
