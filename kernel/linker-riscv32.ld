OUTPUT_ARCH(riscv)
ENTRY(_start)

MEMORY
{
    /* 128MB RAM: OpenSBI occupies 0x80000000-0x802000000; kernel loads at jump address */
    ram (wxa) : ORIGIN = 0x80200000, LENGTH = 126M
}

SECTIONS
{
    /* Boot entry — must be first so OpenSBI jumps directly to _start */
    .text : {
        KEEP(*(.text.boot))
        *(.text .text.*)
    } >ram

    /* Read-only data — page-aligned so .bss (zero-init) is on its own page */
    .rodata : ALIGN(4096) {
        *(.rodata .rodata.*)
        *(.srodata .srodata.*)
    } >ram

    /* ViCell cell manifest (loaded by kernel get_section) */
    __ViCell_manifest : ALIGN(4) {
        *(__ViCell_manifest)
    } >ram

    /* Initialized data */
    .data : ALIGN(4096) {
        . = ALIGN(16);
        __global_pointer$ = . + 0x800;
        *(.data .data.*)
        *(.sdata .sdata.*)
    } >ram

    /* Zero-initialized data — page-aligned to avoid aliasing with .data */
    .bss : ALIGN(4096) {
        __bss_start = .;
        *(.bss .bss.*)
        *(.sbss .sbss.*)
        *(COMMON)
        __bss_end = .;
    } >ram

    /* Kernel stack (32KB) — NOLOAD keeps it out of the binary image */
    .kernel_stack (NOLOAD) : ALIGN(4096) {
        __stack_bottom = .;
        *(.stack)
        . = . + 0x8000;
        __stack_top = .;
    } >ram

    /DISCARD/ : {
        *(.eh_frame)
        *(.rela .rela.*)
    }
}
