/* Linker script for the ViCell silo guest binary.
 *
 * Load address: 0x4000_0000 (Stage-2 fenced region, mapped by VMM).
 * Mailbox page: 0x4000_F000 (final page of the 64 KiB VM mapping).
 * Stack:        4 KiB immediately after .bss, bounded below the mailbox.
 */

ENTRY(_start)
OUTPUT_ARCH(aarch64)

SECTIONS
{
    . = 0x40000000;
    __mailbox_ipa = 0x4000F000;

    .text : {
        *(.text.entry)          /* _start must be at load base */
        *(.text .text.*)
    }

    .rodata : ALIGN(4096) {
        *(.rodata .rodata.*)
    }

    .init_array : ALIGN(8) {
        PROVIDE(__init_array_start = .);
        KEEP(*(.init_array .init_array.*))
        PROVIDE(__init_array_end = .);
    }

    .data : ALIGN(4096) {
        *(.data .data.*)
    }

    .bss : ALIGN(4096) {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        __bss_end = .;
    }

    /* 4 KiB stack placed directly after .bss */
    . = ALIGN(4096);
    _stack_top = . + 0x1000;
    ASSERT(_stack_top <= __mailbox_ipa, "silo guest text/data/stack overlaps mailbox")

    /DISCARD/ : {
        *(.comment)
        *(.eh_frame)
        *(.note.*)
        *(.ARM.*)
    }
}
