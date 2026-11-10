//! AArch64 boot entry point.
//!
//! QEMU virt entered at EL2 stays an EL2 host only with FEAT_VHE; without it
//! the host drops to EL1 and EL2 keeps only the SMCCC stub (`el2_stub`). On
//! BCM the boot CPU retains an EL2 monitor, enters EL1h for the kernel and EL0
//! Cells, and returns to EL2 only via the private HVC gateway. Firmware
//! starting us at EL1 cannot provide that gateway.

use core::arch::global_asm;
use core::sync::atomic::{AtomicU64, Ordering};

use super::backend::{ArmIrqController, ArmPlatform, Board, HostRegime, SecondaryStart};

/// Boot through the Pi EL2 monitor install (`.el2_init`, BSS-clear epilogue).
const BOARD_BCM: usize = <Board as ArmPlatform>::PI_MONITOR_BOOT as usize;

/// Assemble `_secondary_entry` only where secondaries are started by firmware.
const SECONDARY_ENTRY: usize = matches!(
    <Board as ArmPlatform>::SECONDARY_START,
    SecondaryStart::Psci | SecondaryStart::SpinTable
) as usize;

/// Assemble the spin-table trampoline `_spin_table_entry`.
const SECONDARY_SPIN_TABLE: usize = matches!(
    <Board as ArmPlatform>::SECONDARY_START,
    SecondaryStart::SpinTable
) as usize;

/// `SecondaryContext` address `_spin_table_entry` hands `_secondary_entry` in
/// x0: a spin-table release carries no argument, unlike `PSCI_CPU_ON`.
static SPIN_TABLE_CONTEXT: AtomicU64 = AtomicU64::new(0);

/// Publish `context` (the physical address of a cleaned [`SecondaryContext`])
/// for the next core released through the spin table.
///
/// One slot serves every core, so the caller releases cores one at a time and
/// stops at the first one that does not come online.
pub fn publish_spin_table_context(context: u64) {
    SPIN_TABLE_CONTEXT.store(context, Ordering::Relaxed);
    // The released core reads the slot with its caches off.
    super::cache::clean_data_cache_range(SPIN_TABLE_CONTEXT.as_ptr() as usize, 8);
}

/// The EL2 boot path keeps a VHE EL2 host (`HCR_EL2.{E2H,TGE} = {1,1}`).
const HOST_VHE: usize = matches!(<Board as ArmPlatform>::HOST_REGIME, HostRegime::El2Vhe) as usize;

/// `_start` begins with a Linux arm64 `Image` header.
const IMAGE_HEADER: usize = <Board as ArmPlatform>::LINUX_IMAGE_HEADER as usize;

/// Leaving EL2 for an EL1 host first opens the GICv3 system-register interface
/// to EL1 (`ICC_SRE_EL2`); a GICv2-only CPU has no such register.
const GICV3_SRE: usize = <Board as ArmIrqController>::GICV3_CPU_INTERFACE as usize;

/// `Image` header flags (booting.rst): little-endian (bit 0 = 0), 4 KiB pages
/// (bits 2:1 = 1), and bit 3 = 0: the image is not relocatable, so the loader
/// must place it at `text_offset` above a 2 MiB-aligned base at the start of
/// DRAM — on QEMU virt that is exactly the link address.
const IMAGE_FLAGS: usize = 1 << 1;

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
    /// 1 when the boot core is a VHE EL2 host, 0 for an EL1 host: a core the
    /// firmware starts at EL2 must join the same regime or park.
    pub el2_host: u64,
}

impl SecondaryContext {
    /// Capture the boot core's live translation and cache configuration for
    /// `hart_id`, whose kernel stack top is `stack_top`.
    ///
    /// `kernel_root` is passed in rather than read from `TTBR0_EL1`: a Cell's
    /// root may be installed on the calling core, and no secondary may start
    /// under a Cell's translation regime.
    ///
    /// On a VHE host the `_EL1` encodings below are redirected to the EL2&0
    /// regime's registers, so the context carries `MAIR_EL2`/`TCR_EL2`/
    /// `SCTLR_EL2`, which `_secondary_entry` installs through the same
    /// redirection after it sets `HCR_EL2.E2H`.
    pub fn for_hart(hart_id: u64, stack_top: u64, kernel_root: u64) -> Self {
        let mair: u64;
        let tcr: u64;
        let sctlr: u64;
        // SAFETY: MAIR/TCR/SCTLR (EL1, or EL2 under VHE) are read-only here.
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
            el2_host: super::el2::is_el2() as u64,
        }
    }
}

const CTX_HART_ID: usize = core::mem::offset_of!(SecondaryContext, hart_id);
const CTX_STACK_TOP: usize = core::mem::offset_of!(SecondaryContext, stack_top);
const CTX_MAIR: usize = core::mem::offset_of!(SecondaryContext, mair);
const CTX_TCR: usize = core::mem::offset_of!(SecondaryContext, tcr);
const CTX_TTBR0: usize = core::mem::offset_of!(SecondaryContext, ttbr0);
const CTX_SCTLR: usize = core::mem::offset_of!(SecondaryContext, sctlr);
const CTX_EL2_HOST: usize = core::mem::offset_of!(SecondaryContext, el2_host);

global_asm!(
    r#"
    // Leave EL2 for an EL1h host at \target, DAIF masked: HCR_EL2.RW only
    // (TGE=0, no traps, no Stage-2), FP/SIMD and the EL1 physical counter and
    // timer untrapped (non-VHE CPTR_EL2/CNTHCTL_EL2 layouts).
    .macro cellos_el2_to_el1 tmp, target
    mov \tmp, #(1 << 31)       // HCR_EL2.RW=1, TGE=0: EL1 is AArch64
    msr hcr_el2, \tmp
    mov \tmp, #0x33ff          // CPTR_EL2 RES1 bits; no FP/SIMD traps to EL2
    msr cptr_el2, \tmp
    mov \tmp, #3               // EL1PCTEN | EL1PCEN
    msr cnthctl_el2, \tmp
    msr cntvoff_el2, xzr
    .if {gicv3_sre}
    // ICC_SRE_EL2 Enable|DIB|DFB|SRE: EL1 may use the ICC_* registers.
    mrs \tmp, icc_sre_el2
    orr \tmp, \tmp, #0xf
    msr icc_sre_el2, \tmp
    .endif
    mov \tmp, #0x3c5           // EL1h with D/A/I/F masked
    msr spsr_el2, \tmp
    adr \tmp, \target
    msr elr_el2, \tmp
    isb
    eret
    .endm

    .section .text.boot
    .global _start
    .balign 4
_start:
    .if {image_header}
    // Linux arm64 Image header (booting.rst), 64 bytes; the loader enters at
    // offset 0 with x0 = FDT physical address, which the path below preserves.
    b    .Lentry                  // code0
    .long 0                       // code1
    .quad __image_text_offset     // text_offset (linker-aarch64.ld)
    .quad __image_size            // image_size, BSS and boot stack included
    .quad {image_flags}           // flags
    .quad 0                       // res2
    .quad 0                       // res3
    .quad 0                       // res4
    .ascii "ARM\x64"              // magic
    .long 0                       // res5: no PE/COFF header
.Lentry:
    .endif
    // Park secondary CPUs immediately.
    // QEMU raspi3b boots all 4 Cortex-A53 cores simultaneously; without this gate
    // they all execute kmain in parallel, corrupting BSS, frame allocator, paging,
    // SDHCI probe state, and UART output.
    // BCM2836 MPIDR_EL1[7:0] = Aff0 = core index (0–3).  Only the core whose
    // whole affinity (Aff3 at 39:32, Aff2..Aff0 at 23:0) is zero proceeds:
    // Aff0 alone repeats in every cluster, so a cluster-1 core 0 would pass.
    // Two TSTs because 0xFF_00FF_FFFF is not a logical immediate.
    mrs  x1, mpidr_el1
    tst  x1, #0xFFFFFF          // Aff2..Aff0
    b.ne .Lsecondary_park       // non-zero → secondary core → park forever
    tst  x1, #0xFF00000000      // Aff3
    b.ne .Lsecondary_park

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
    .if {board_bcm}
    // Pi: the host and its Cells run at EL1/EL0. Keep EL2 as a private
    // monitor for Stage-2 and guest world switches.
    mov x20, #1               // preserve EL2 boot provenance across ERET
    adrp x0, __pi_monitor_stack_top
    add  x0, x0, :lo12:__pi_monitor_stack_top
    mov sp, x0                // dedicated SP_EL2, separate from EL1 host
    adrp x0, __pi_monitor_vectors
    add  x0, x0, :lo12:__pi_monitor_vectors
    msr vbar_el2, x0
    msr tpidr_el2, xzr
    cellos_el2_to_el1 x0, .el1_entry
    .else
    // An EL2 host needs FEAT_VHE. With HCR_EL2.TGE = 1 and E2H = 0 the EL1&0
    // regime treats SCTLR_EL1.M as 0 for EL0 (ARM ARM AArch64.S1Enabled), so
    // EL0 Cells would run untranslated with no AP/UXN checks. E2H goes in
    // first and is read back; TGE follows only once E2H holds, so that pair
    // is never live, not even between two writes.
    // SAFETY: we are at EL2; HCR_EL2 is EL2-private.
    mov x0, #(1 << 31)       // RW: an EL1 guest runs AArch64
    orr x0, x0, #(1 << 34)   // E2H: kernel and Cells share the EL2&0 regime
    msr hcr_el2, x0
    isb
    mrs x0, hcr_el2
    tbz x0, #34, .Lel2_no_vhe // no FEAT_VHE: E2H reads back 0
    orr x0, x0, #(1 << 27)   // TGE: EL0 exceptions are taken to VBAR_EL2
    msr hcr_el2, x0
    isb

    // TPIDR_EL2 is the per-CPU live-vCPU marker. Firmware does not guarantee
    // its reset value, so clear it before any normal Cell can trap from EL0.
    // vcpu_enter_guest is the only path allowed to make it non-zero.
    msr tpidr_el2, xzr

    // E2H layout: CPTR_EL2.FPEN = 0b11 leaves FP/SIMD untrapped at EL2 and EL0.
    mov x0, #(3 << 20)
    msr cptr_el2, x0
    // E2H layout: CNTHCTL_EL2.EL1PCTEN|EL1PCEN keep a guest's physical
    // counter and timer untrapped; the EL0 enables stay clear.
    mov x0, #(3 << 10)
    msr cnthctl_el2, x0
    msr cntvoff_el2, xzr
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
    bl   el2_mark_active
    mov  x0, #0             // hartid = 0
    mov  x1, x19            // DTB pointer
    bl   kmain

    // If kmain returns, halt.
3:
    wfi
    b    3b

.Lel2_no_vhe:
    // No FEAT_VHE: the host runs at EL1 and EL2 keeps only the SMCCC stub
    // (`el2_stub`), so an HVC from the host fails cleanly. No hypervisor.
    mov x20, #1               // tells `.el1_entry` the host left EL2
    adrp x0, __el2_stub_vectors
    add  x0, x0, :lo12:__el2_stub_vectors
    msr vbar_el2, x0
    cellos_el2_to_el1 x0, .el1_entry
    .endif

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
    cbz x20, 7f               // x20 = 1: the host left EL2 for EL1 above
    .if {board_bcm}
    mov x0, x21              // original exception level passed to Rust
    bl el2_mark_monitor_boot
    .else
    bl el2_mark_el1_fallback
    .endif
7:
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

    .if {secondary_entry}
    // ── Secondary core entry (PSCI_CPU_ON) ───────────────────────────────────
    // x0 = &SecondaryContext, a *physical* address: firmware starts the core
    // with the MMU off, caches off, interrupts masked, at EL1 — or at EL2 on a
    // `virtualization=on` machine. This runs from the identity-mapped image,
    // so control can flow straight into Rust once the boot core's translation
    // regime is installed.
    .global _secondary_entry
    .balign 4
_secondary_entry:
    msr  daifset, #0xf
    .if {host_vhe}
    // Join the boot core's regime (`SecondaryContext::el2_host`) or park; the
    // boot core's bounded wait then keeps the system single-hart.
    ldr  x10, [x0, #{ctx_el2_host}]
    mrs  x9, CurrentEL
    cmp  x9, #(2 << 2)
    b.eq 9f
    cbnz x10, .Lsecondary_park  // an EL2 host cannot take a core started at EL1
    b    8f
9:
    cbz  x10, .Lsecondary_to_el1
    // VHE host, set up exactly as `.el2_init`: E2H read back before TGE, so
    // TGE = 1 with E2H = 0 is never live. From here every `_EL1` access
    // below is redirected to its EL2&0 counterpart, so the shared sequence
    // installs the boot core's EL2 translation regime and CPTR_EL2.FPEN.
    mov  x9, #(1 << 31)
    orr  x9, x9, #(1 << 34)
    msr  hcr_el2, x9
    isb
    mrs  x9, hcr_el2
    tbz  x9, #34, .Lsecondary_park
    orr  x9, x9, #(1 << 27)
    msr  hcr_el2, x9
    isb
    msr  tpidr_el2, xzr
    mov  x9, #(3 << 10)
    msr  cnthctl_el2, x9
    msr  cntvoff_el2, xzr
    b    8f
.Lsecondary_to_el1:
    // The boot core runs an EL1 host (no FEAT_VHE); leave EL2 the same way.
    adrp x9, __el2_stub_vectors
    add  x9, x9, :lo12:__el2_stub_vectors
    msr  vbar_el2, x9
    cellos_el2_to_el1 x9, 8f
8:
    .else
    // An EL1 host (the Pi monitor boot): firmware may start the core at EL2
    // (the Pi armstub's spin table, TF-A's PSCI). Leave EL2 as `.Lel2_no_vhe`
    // does, with only the SMCCC stub behind it — the Pi monitor serves the
    // boot core alone.
    mrs  x9, CurrentEL
    cmp  x9, #(2 << 2)
    b.ne 8f
    adrp x9, __el2_stub_vectors
    add  x9, x9, :lo12:__el2_stub_vectors
    msr  vbar_el2, x9
    cellos_el2_to_el1 x9, 8f
8:
    .endif
    .if {board_bcm}
    // As `.el1_entry`: EL0 Cells on this core read the architected counters.
    mov  x9, #0x101
    msr  cntkctl_el1, x9
    isb
    .endif
    // Exceptions to EL1 use SP_EL1 (SP_EL2 on a VHE host at EL2); set that
    // before touching SP.
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

    .if {secondary_spin_table}
    // ── Secondary core entry (spin-table release) ────────────────────────────
    // Firmware branches here with the MMU off and x0 = 0; load the context the
    // boot core published, then share the PSCI path.
    .global _spin_table_entry
    .balign 4
_spin_table_entry:
    adrp x0, {spin_ctx}
    ldr  x0, [x0, :lo12:{spin_ctx}]
    b    _secondary_entry
    .endif
    .endif
    "#,
    board_bcm = const BOARD_BCM,
    secondary_entry = const SECONDARY_ENTRY,
    secondary_spin_table = const SECONDARY_SPIN_TABLE,
    spin_ctx = sym SPIN_TABLE_CONTEXT,
    host_vhe = const HOST_VHE,
    ctx_el2_host = const CTX_EL2_HOST,
    image_header = const IMAGE_HEADER,
    image_flags = const IMAGE_FLAGS,
    ctx_hart = const CTX_HART_ID,
    ctx_stack = const CTX_STACK_TOP,
    ctx_mair = const CTX_MAIR,
    ctx_tcr = const CTX_TCR,
    ctx_ttbr0 = const CTX_TTBR0,
    ctx_sctlr = const CTX_SCTLR,
    gicv3_sre = const GICV3_SRE,
);
