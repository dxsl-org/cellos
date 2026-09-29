//! AArch64 boot entry point.
//!
//! QEMU virt remains an EL2 host. On BCM the boot CPU retains an EL2 monitor,
//! enters EL1h for the kernel and EL0 Cells, and returns to EL2 only via the
//! private HVC gateway. Firmware starting us at EL1 cannot provide that gateway.

use core::arch::global_asm;

const BOARD_BCM: usize = cfg!(any(feature = "board-rpi3", feature = "board-rpi4")) as usize;

/// Boot state a secondary core needs, published by the boot core before
/// `PSCI_CPU_ON` and read by `_secondary_entry` with the MMU **still off**.
///
/// The translation and cache-control registers are copied from the boot core's
/// live values rather than recomputed: a secondary must run with exactly the
/// configuration the running kernel was built for (T0SZ, MAIR indexes, and the
/// `SCTLR_EL1` bits `CFI`/`MTE`/`Arch::init()` set), and re-deriving them here
/// would be a second source of truth for the same page tables.
///
/// Written once by the boot core, cleaned to the point of coherency, and read
/// by a core whose caches are off — hence the fixed offsets, which the assembly
/// consumes through `const` operands so they cannot drift from this layout.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SecondaryContext {
    /// Logical hart id to run as (delivered to the Rust entry point in x0).
    pub hart_id: u64,
    /// Top of this hart's kernel stack (16-byte aligned).
    pub stack_top: u64,
    /// `MAIR_EL1` — memory attribute indirection.
    pub mair: u64,
    /// `TCR_EL1` — translation control.
    pub tcr: u64,
    /// `TTBR0_EL1` — the kernel's root table (physical) with its ASID.
    pub ttbr0: u64,
    /// `SCTLR_EL1` — the boot core's live system control (M/C/I + hardening).
    pub sctlr: u64,
}

impl SecondaryContext {
    /// Capture the boot core's live translation and cache configuration for
    /// `hart_id`, whose kernel stack top is `stack_top`.
    ///
    /// `kernel_root` is passed in rather than read from `TTBR0_EL1`: a Cell's
    /// root may be installed on the calling core, and no secondary may start
    /// under a Cell's translation regime.
    pub fn for_hart(hart_id: u64, stack_top: u64, kernel_root: u64) -> Self {
        let mair: u64;
        let tcr: u64;
        let sctlr: u64;
        // SAFETY: MAIR/TCR/SCTLR_EL1 are EL1-private and read-only here.
        unsafe {
            core::arch::asm!(
                "mrs {mair}, mair_el1",
                "mrs {tcr},  tcr_el1",
                "mrs {sctlr}, sctlr_el1",
                mair = out(reg) mair,
                tcr = out(reg) tcr,
                sctlr = out(reg) sctlr,
                options(nomem, nostack)
            );
        }
        Self {
            hart_id,
            stack_top,
            mair,
            tcr,
            ttbr0: kernel_root,
            sctlr,
        }
    }
}

const CTX_HART_ID: usize = core::mem::offset_of!(SecondaryContext, hart_id);
const CTX_STACK_TOP: usize = core::mem::offset_of!(SecondaryContext, stack_top);
const CTX_MAIR: usize = core::mem::offset_of!(SecondaryContext, mair);
const CTX_TCR: usize = core::mem::offset_of!(SecondaryContext, tcr);
const CTX_TTBR0: usize = core::mem::offset_of!(SecondaryContext, ttbr0);
const CTX_SCTLR: usize = core::mem::offset_of!(SecondaryContext, sctlr);

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

    // QEMU's `-kernel` ELF loader can enter at EL3 whereas its raw Linux
    // image loader normally enters EL2. Preserve this actual provenance.
    mrs x0, CurrentEL
    lsr x0, x0, #2
    mov x21, x0
    mov x20, #0               // EL1-only firmware: no retained monitor
    cmp x0, #3
    b.eq .el3_to_el2
    cmp x0, #2
    b.eq .el2_init
    b .el1_entry

.el3_to_el2:
    // EL3 direct ELF boot: enter non-secure AArch64 EL2 before touching any
    // EL2-private register. SCR.HCE enables the EL1 host's HVC conduit.
    mov x0, #0x501           // SCR_EL3.RW | HCE | NS
    msr scr_el3, x0
    msr cptr_el3, xzr        // do not trap lower-level FP/SIMD into EL3
    mov x0, #0x3c9           // EL2h, DAIF masked
    msr spsr_el3, x0
    adr x0, .el2_init
    msr elr_el3, x0
    isb
    eret

.el2_init:
    // Non-VHE EL0 Cells require the host at EL1. Keep EL2 as a private
    // monitor for Stage-2 and guest world switches.
    .if {board_bcm}
    mov x20, #1               // preserve EL2 boot provenance across ERET
    adrp x0, __pi_monitor_stack_top
    add  x0, x0, :lo12:__pi_monitor_stack_top
    mov sp, x0                // dedicated SP_EL2, separate from EL1 host
    adrp x0, __pi_monitor_vectors
    add  x0, x0, :lo12:__pi_monitor_vectors
    msr vbar_el2, x0
    msr tpidr_el2, xzr
    mov x0, #(1 << 31)       // HCR_EL2.RW=1, TGE=0: EL1 is AArch64
    msr hcr_el2, x0
    mov x0, #0x33ff          // CPTR_EL2 RES1 bits; no FP/SIMD traps to EL2
    msr cptr_el2, x0
    mov x0, #3               // EL1PCTEN | EL1PCEN
    msr cnthctl_el2, x0
    msr cntvoff_el2, xzr
    mov x0, #0x3c5           // EL1h with D/A/I/F masked
    msr spsr_el2, x0
    adr x0, .el1_entry
    msr elr_el2, x0
    isb
    eret
    .endif

    // F2: set HCR_EL2 = RW(1<<31) | TGE(1<<27) FIRST.
    // TGE routes EL0 exceptions to VBAR_EL2 — required for Cell SVCs at EL2 host.
    // RW ensures any future EL1 guest runs AArch64 (also harmless now).
    // SAFETY: we are at EL2; HCR_EL2 is EL2-private.
    mov x0, #(1 << 31)
    orr x0, x0, #(1 << 27)
    msr hcr_el2, x0
    isb

    // TPIDR_EL2 is the per-CPU live-vCPU marker. Firmware does not guarantee
    // its reset value, so clear it before any normal Cell can trap from EL0.
    // vcpu_enter_guest is the only path allowed to make it non-zero.
    msr tpidr_el2, xzr

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

    // Pi host: let EL0 Cells read the architected counters
    // (CNTKCTL_EL1.EL0PCTEN | .EL0VTEN). Without these the BCM host timer
    // service's `MRS CNTPCT_EL0` faults from EL0 into the EL1 kernel vector
    // and panics the kernel; the guest world-switch saves and restores this
    // register, so a Cell's access keeps working after a Tier 3 guest runs.
    .if {board_bcm}
    mov x0, #0x101
    msr cntkctl_el1, x0
    isb
    .endif

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
    .if {board_bcm}
    cbz x20, 7f
    mov x0, x21              // original exception level passed to Rust
    bl el2_mark_monitor_boot
7:
    .endif
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
    // with core 0's boot sequence.  Firmware that holds its secondaries off
    // (QEMU virt, which starts them only on PSCI_CPU_ON) never reaches this.
.Lsecondary_park:
    msr  daifset, #0xf          // mask all interrupts (prevent spurious wake)
    wfi
    b    .Lsecondary_park

    .if {secondary_psci}
    // ── Secondary core entry (PSCI_CPU_ON) ───────────────────────────────────
    // x0 = &SecondaryContext, a *physical* address: firmware starts the core
    // with the MMU off, caches off, at EL1, interrupts masked. This runs from
    // the identity-mapped image, so control can flow straight into Rust once
    // the boot core's translation regime is installed.
    .global _secondary_entry
    .balign 4
_secondary_entry:
    msr  daifset, #0xf
    // Exceptions to EL1 use SP_EL1; set that before touching SP.
    msr  spsel, #1
    isb
    mov  x9, #(3 << 20)         // CPACR_EL1.FPEN: FP/SIMD at EL1 and EL0
    msr  cpacr_el1, x9
    isb
    ldr  x9, [x0, #{ctx_stack}]
    mov  sp, x9
    ldr  x9, [x0, #{ctx_mair}]
    msr  mair_el1, x9
    ldr  x9, [x0, #{ctx_tcr}]
    msr  tcr_el1, x9
    ldr  x9, [x0, #{ctx_ttbr0}]
    msr  ttbr0_el1, x9
    dsb  sy
    isb
    tlbi vmalle1                // this PE only: no other hart's entries
    dsb  nsh
    isb
    ldr  x9, [x0, #{ctx_sctlr}]
    msr  sctlr_el1, x9          // enables MMU/caches exactly as the boot core runs
    dsb  sy
    isb
    // Identity-mapped, so the context is still readable here; hand the logical
    // hart id to the Rust entry point and never return.
    ldr  x0, [x0, #{ctx_hart}]
    b    smp_aarch64_secondary_main
    .endif
    "#,
    board_bcm = const BOARD_BCM,
    secondary_psci = const (!cfg!(feature = "board-rpi3") as usize),
    ctx_hart = const CTX_HART_ID,
    ctx_stack = const CTX_STACK_TOP,
    ctx_mair = const CTX_MAIR,
    ctx_tcr = const CTX_TCR,
    ctx_ttbr0 = const CTX_TTBR0,
    ctx_sctlr = const CTX_SCTLR,
);
