/* Linker Script for RISC-V ViOS Kernel (QEMU virt machine) */

OUTPUT_ARCH(riscv)
ENTRY(_start)

MEMORY
{
    /* QEMU virt machine memory layout */
    /* OpenSBI uses 0x80000000 - 0x80200000 */
    RAM : ORIGIN = 0x80200000, LENGTH = 126M
}

SECTIONS
{
    . = 0x80200000;

    .text : {
        *(.text.boot)  /* Boot code first */
        *(.text .text.*)
    } > RAM

    .rodata : {
        *(.rodata .rodata.*)
    } > RAM

    .data : {
        *(.data .data.*)
    } > RAM

    .bss : {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        __bss_end = .;
    } > RAM

    /* Stack grows downward */
    . = ALIGN(16);
    __stack_start = .;
    . += 0x10000; /* 64KB stack */
    __stack_end = .;

    /* Heap for allocator */
    . = ALIGN(16);
    __heap_start = .;
    . += 0x100000; /* 1MB heap */
    __heap_end = .;

    /DISCARD/ : {
        *(.eh_frame)
    }
}
