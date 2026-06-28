//! AArch64 boot entry point.
//!
//! Two EL paths share a single entry:
//!   - EL2 (QEMU `virtualization=on`): `.el2_init` stays at EL2, sets HCR_EL2,
//!     calls `el2_mark_active`, then `kmain`.
//!   - EL1 (default QEMU without `-machine virt,virtualization=on`): `.el1_entry`
//!     runs the existing EL1 setup and calls `kmain` without marking EL2.
//!
//! Both paths share BSS clearing and stack setup; the EL1 path does those steps
//! itself while the EL2 path folds them into `.el2_init`.

use core::arch::global_asm;

global_asm!(
    r#"
    .section .text.boot
    .global _start
    .balign 4
_start:
    // Park secondary CPUs immediately.
    // QEMU raspi3b boots all 4 Cortex-A53 cores simultaneously; without this gate
    // they all execute kmain in parallel, corrupting BSS, frame allocator, paging,
    // SDHCI probe state, and UART output.
    // BCM2836 MPIDR_EL1[7:0] = Aff0 = core index (0–3).  Only core 0 proceeds.
    mrs  x1, mpidr_el1
    and  x1, x1, #0xFF          // extract Aff0 (CPU index within cluster)
    cbnz x1, .Lsecondary_park   // non-zero → secondary core → park forever

    // Disable all interrupts (DAIF = 0b1111).
    msr daifset, #0xf

    // Stash DTB pointer (x0 on QEMU virt) in x19 (callee-saved) before it
    // is clobbered by the BSS-clear loop and stack setup.
    mov  x19, x0  // DTB physical address

    // Determine current exception level.
    mrs x0, CurrentEL
    lsr x0, x0, #2          // CurrentEL[3:2]
    cmp x0, #2
    b.eq .el2_init
    b .el1_entry             // Already in EL1

.el2_init:
    // F2: set HCR_EL2 = RW(1<<31) | TGE(1<<27) FIRST.
    // TGE routes EL0 exceptions to VBAR_EL2 — required for Cell SVCs at EL2 host.
    // RW ensures any future EL1 guest runs AArch64 (also harmless now).
    // SAFETY: we are at EL2; HCR_EL2 is EL2-private.
    mov x0, #(1 << 31)
    orr x0, x0, #(1 << 27)
    msr hcr_el2, x0
    isb

    // Enable FP/SIMD at EL2 host (CPTR_EL2=0 disables all traps).
    msr cptr_el2, xzr
    isb

    // Set SP_EL2 stack.
    adrp x0, __stack_top
    add  x0, x0, :lo12:__stack_top
    mov  sp, x0

    // Clear BSS.
    adrp x0, __bss_start
    add  x0, x0, :lo12:__bss_start
    adrp x1, __bss_end
    add  x1, x1, :lo12:__bss_end
1:
    cmp  x0, x1
    b.hs 2f
    str  xzr, [x0], #8
    b    1b
2:
    // UART sentinel 'E': on QEMU virt this reaches PL011 and confirms EL2 init.
    // On board-rpi3 (MMU off, no PL011) this writes to RAM@0x09000000 — harmless.
    mov  x0, #0x09000000
    mov  w1, #0x45          // ASCII 'E'
    strb w1, [x0]

    // Mark EL2_ACTIVE = true and jump to kmain.
    bl   el2_mark_active
    mov  x0, #0             // hartid = 0
    mov  x1, x19            // DTB pointer
    bl   kmain

    // If kmain returns, halt.
3:
    wfi
    b    3b

.el1_entry:
    // Enable FP/SIMD in EL1 and EL0 (CPACR_EL1.FPEN = 0b11).
    // Without this, any FP/SIMD instruction traps with EC=0x07.
    mov x0, #(3 << 20)
    msr cpacr_el1, x0
    isb

    // Force EL1h mode: exceptions taken to EL1 use SP_EL1 (not SP_EL0).
    // QEMU raspi3b boots at EL1 and may leave PSTATE.SPSEL=0 (EL1t), meaning
    // SP_EL1 stays at the unknown reset value.  Any EL0→EL1 exception would
    // then crash on its first `sub sp, sp, #N` because SP_EL1 is garbage.
    // Setting SPSEL=1 before the stack `mov sp, x0` makes `mov sp` write to
    // SP_EL1, so both the kernel and exception handlers share a valid stack.
    msr spsel, #1
    isb

    // Set up initial stack at __stack_top (now writes SP_EL1 since SPSEL=1).
    adrp x0, __stack_top
    add  x0, x0, :lo12:__stack_top
    mov  sp, x0

    // Clear BSS section.
    adrp x0, __bss_start
    add  x0, x0, :lo12:__bss_start
    adrp x1, __bss_end
    add  x1, x1, :lo12:__bss_end
4:
    cmp  x0, x1
    b.hs 5f
    str  xzr, [x0], #8
    b    4b
5:
    // Jump to Rust kmain(hartid=0, dtb=x19).
    mov  x0, #0             // hartid (CPU 0)
    mov  x1, x19            // DTB pointer stashed from entry x0
    bl   kmain

    // If kmain returns, halt.
6:
    wfi
    b    6b

    // Secondary CPU park: interrupts masked, loop on WFI forever.
    // QEMU raspi3b boots cores 1–3 here; they yield the CPU and never interfere
    // with core 0's boot sequence.  Future SMP bringup can replace this with a
    // spin-table or PSCI-based wake loop.
.Lsecondary_park:
    msr  daifset, #0xf          // mask all interrupts (prevent spurious wake)
    wfi
    b    .Lsecondary_park
    "#
);
