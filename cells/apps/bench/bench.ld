OUTPUT_ARCH(riscv)
ENTRY(_start)

SECTIONS
{
    /* Benchmark suite user-space base address.
     * Placed at 192MB — above the compositor (160MB) and clear of every other
     * cell.  The bench cell has a multi-MB writable segment (sample Vecs +
     * default LLD BSS); without a fixed base its writable segment landed across
     * vaddr 0x400000 and clobbered init's .text in the shared single address
     * space, faulting init (scause=0xc) the moment it was scheduled again.
     * Every cell MUST occupy a disjoint VA window because ViCell maps all cells
     * into one shared KERNEL_ROOT page table (SAS). */
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

    .vicell_manifest : ALIGN(8) {
        KEEP(*(__ViCell_manifest))
    }
}
