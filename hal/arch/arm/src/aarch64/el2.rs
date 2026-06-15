//! AArch64 EL2 host mode support.
//!
//! This module is compiled only for the `aarch64` target; all public symbols
//! that need to be visible from assembly are `#[no_mangle]`.
//!
//! Boot sequence (EL2 path):
//!   boot.rs: .el2_init → bl el2_mark_active → bl kmain
//!   kmain: paging::activate() → el2_mmu_init()
//!          trap::init()       → msr vbar_el2, __vectors_el2
//!          timer::init()      → CNTHP_* + enable_irq(26)

use core::arch::global_asm;
use core::sync::atomic::{AtomicBool, Ordering};

/// Set by `el2_mark_active()` at boot; read by `is_el2()` and assembly thunks.
///
/// `AtomicBool` is 1 byte on AArch64 — `ldrb w9, [x9]` in assembly works correctly.
/// Named exactly `EL2_ACTIVE` because the assembly trampolines reference it by symbol name via ADRP.
#[no_mangle]
pub static EL2_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Returns `true` if the kernel booted at EL2 (QEMU `virtualization=on`).
#[inline]
pub fn is_el2() -> bool {
    EL2_ACTIVE.load(Ordering::Relaxed)
}

/// Called from boot.rs assembly (`bl el2_mark_active`) before `bl kmain`.
///
/// # Safety
/// Must be called exactly once at boot before any concurrent access to `EL2_ACTIVE`.
#[no_mangle]
pub extern "C" fn el2_mark_active() {
    EL2_ACTIVE.store(true, Ordering::Relaxed);
}

/// Set HCR_EL2 = RW(1<<31) | TGE(1<<27).
///
/// - TGE: routes EL0 exceptions to VBAR_EL2 (mandatory for Cell SVCs at EL2 host).
/// - RW: EL1 guest executes in AArch64 mode (required for P03 guest; harmless now).
///
/// # Safety
/// Must be called at EL2 during early boot before any EL0 activity.
#[inline]
pub unsafe fn el2_hcr_init() {
    // SAFETY: HCR_EL2 is EL2-private; caller guarantees we are at EL2.
    unsafe {
        core::arch::asm!(
            "mov {v}, #(1 << 31)",
            "orr {v}, {v}, #(1 << 27)",
            "msr hcr_el2, {v}",
            "isb",
            v = out(reg) _,
            options(nomem, nostack),
        );
    }
}

/// Activate the EL2 MMU with the given L1 page table root.
///
/// TCR_EL2 non-VHE encoding: bit31 and bit23 are RES1 (ARMv8.0 requirement).
/// MAIR_EL2 mirrors the EL1 layout (index0=Device, index1=Normal-WB).
///
/// A UART sentinel byte `'M'` (0x4D) is written to PL011 at 0x09000000 just
/// before `SCTLR_EL2.M` is set, confirming the instruction stream reached that
/// point even if the MMU-on fault prevents any subsequent UART traffic.
///
/// # Safety
/// - `ttbr0_phys` must identity-cover the current PC and all page-table frames.
/// - Must be called at EL2 after `el2_hcr_init()`.
pub unsafe fn el2_mmu_init(ttbr0_phys: u64) {
    // Same MAIR as EL1: Device-nGnRnE at index 0, Normal-WB-WA at index 1.
    let mair: u64 = 0x0000_0000_0000_FF00;
    // TCR_EL2 (non-VHE):
    //   T0SZ=25  → 39-bit VA
    //   IRGN0=WB-WA-RA (bits 9:8 = 0b01)
    //   ORGN0=WB-WA-RA (bits 11:10 = 0b01)
    //   SH0=Inner-shareable (bits 13:12 = 0b11)
    //   TG0=4 KB (bits 15:14 = 0b00)
    //   bit23 = RES1 (ARMv8.0 non-VHE requirement)
    //   bit31 = RES1 (ARMv8.0 non-VHE requirement)
    let tcr: u64 = 25_u64
        | (1 << 8)       // IRGN0 = WB-WA-RA
        | (1 << 10)      // ORGN0 = WB-WA-RA
        | (3 << 12)      // SH0   = Inner-shareable
        | (0 << 14)      // TG0   = 4 KB
        | (1 << 23)      // RES1
        | (1_u64 << 31); // RES1
    // SAFETY: EL2-private registers; identity-map covers current PC; caller verified EL2.
    unsafe {
        core::arch::asm!(
            "msr mair_el2, {mair}",
            "msr tcr_el2,  {tcr}",
            "isb",
            "msr ttbr0_el2, {ttbr0}",
            "dsb sy",
            "isb",
            "tlbi alle2",          // invalidate all EL2 TLB entries
            "dsb nsh",
            "isb",
            // Sentinel 'M' before MMU-on — visible on UART even if SCTLR write faults.
            "mov {uart}, #0x09000000",
            "mov {b}, #0x4D",      // ASCII 'M'
            "strb {b:w}, [{uart}]",
            // Enable MMU: SCTLR_EL2.M=1 (bit0), .C=1 (bit2), .I=1 (bit12).
            "mrs {scr}, sctlr_el2",
            "orr {scr}, {scr}, #(1 << 0)",
            "orr {scr}, {scr}, #(1 << 2)",
            "orr {scr}, {scr}, #(1 << 12)",
            "msr sctlr_el2, {scr}",
            "dsb sy",
            "isb",
            mair  = in(reg) mair,
            tcr   = in(reg) tcr,
            ttbr0 = in(reg) ttbr0_phys,
            uart  = out(reg) _,
            b     = out(reg) _,
            scr   = out(reg) _,
            options(nostack),
        );
    }
}

// ── EL2 context switch ────────────────────────────────────────────────────────
//
// `__switch_el2` mirrors `__switch_el1` (context.rs global_asm) but reads/writes
// `elr_el2`/`spsr_el2` instead of `elr_el1`/`spsr_el1`.
// CpuContext offsets (see context.rs):
//   x19-x30: 0..88   sp: 96   elr: 104   spsr: 112   sp_el0: 120   daif: 128

global_asm!(r#"
    .section .text
    .global __switch_el2
    .balign 4
__switch_el2:
    // x0 = old CpuContext*, x1 = new CpuContext*
    stp  x19, x20, [x0, #0]
    stp  x21, x22, [x0, #16]
    stp  x23, x24, [x0, #32]
    stp  x25, x26, [x0, #48]
    stp  x27, x28, [x0, #64]
    stp  x29, x30, [x0, #80]
    mov  x9,  sp
    str  x9,       [x0, #96]
    mrs  x9,  elr_el2
    mrs  x10, spsr_el2
    stp  x9,  x10, [x0, #104]
    // SP_EL0 is banked and NOT saved by the CPU on exception entry.
    mrs  x9,  sp_el0
    str  x9,       [x0, #120]
    mrs  x9,  daif
    str  x9,       [x0, #128]

    ldp  x19, x20, [x1, #0]
    ldp  x21, x22, [x1, #16]
    ldp  x23, x24, [x1, #32]
    ldp  x25, x26, [x1, #48]
    ldp  x27, x28, [x1, #64]
    ldp  x29, x30, [x1, #80]
    ldr  x9,       [x1, #96]
    mov  sp,  x9
    ldp  x9,  x10, [x1, #104]
    msr  elr_el2,  x9
    msr  spsr_el2, x10
    ldr  x9,       [x1, #120]
    msr  sp_el0,   x9
    ldr  x9,       [x1, #128]
    msr  daif,     x9
    ret
"#);

// ── EL2 vector table ─────────────────────────────────────────────────────────
//
// `__vectors_el2` is a 2048-byte aligned VBAR_EL2 table.
// Layout: 16 slots × 0x80, same as __vectors (trap.rs) but using _el2 sysregs
// and unique trampoline names to avoid duplicate symbol linker errors.
//
// TrapFrame layout (same as EL1, 35 × 8 = 280 bytes):
//   x0..x30  → offsets 0..240
//   elr_el1  → offset 248  (named elr_el1 in the struct; holds elr_el2 at runtime)
//   spsr_el1 → offset 256
//   far_el1  → offset 264
//   esr_el1  → offset 272
// The field names are "wrong" at EL2 but the struct is just a bag of u64 — the
// Rust handler reads frame.esr_el1 which holds ESR_EL2's value at runtime.

global_asm!(r#"
    .section .text.vectors
    .global __vectors_el2
    .balign 2048
__vectors_el2:
    // ── Current EL, SP_EL0 ──────────────────────────────────────────────────
    .balign 0x80; b vt_sync_el2_cur
    .balign 0x80; b vt_irq_el2_cur
    .balign 0x80; b vt_sync_el2_cur    // FIQ → treat as sync
    .balign 0x80; b vt_sync_el2_cur    // SError → treat as sync
    // ── Current EL, SP_ELx ──────────────────────────────────────────────────
    .balign 0x80; b vt_sync_el2_cur
    .balign 0x80; b vt_irq_el2_cur
    .balign 0x80; b vt_sync_el2_cur
    .balign 0x80; b vt_sync_el2_cur
    // ── Lower EL (AArch64) — Cell SVCs and timer IRQs ───────────────────────
    .balign 0x80; b vt_sync_el2_lower
    .balign 0x80; b vt_irq_el2_lower
    .balign 0x80; b vt_sync_el2_lower  // FIQ from lower-EL
    .balign 0x80; b vt_sync_el2_lower  // SError from lower-EL
    // ── Lower EL (AArch32) — not supported ──────────────────────────────────
    .balign 0x80; b .
    .balign 0x80; b .
    .balign 0x80; b .
    .balign 0x80; b .

    // ── Out-of-line trampolines ──────────────────────────────────────────────
    // vt_sync_el2_cur: always the SVC/Cell-fault host path (current-EL traps
    //   cannot be guest traps — no TPIDR_EL2 check needed).
    // vt_sync_el2_lower: lower-EL sync (Cell SVCs or guest EL1 traps).
    //   Checks TPIDR_EL2: if non-zero a vCPU is running → jump to vt_vcpu_trap
    //   (defined in vcpu.rs global_asm! block); otherwise fall through to host SVC.
    .section .text
    .balign 4
vt_sync_el2_cur:
    sub  sp, sp, #(35 * 8)
    stp  x0,  x1,  [sp, #0]
    stp  x2,  x3,  [sp, #16]
    stp  x4,  x5,  [sp, #32]
    stp  x6,  x7,  [sp, #48]
    stp  x8,  x9,  [sp, #64]
    stp  x10, x11, [sp, #80]
    stp  x12, x13, [sp, #96]
    stp  x14, x15, [sp, #112]
    stp  x16, x17, [sp, #128]
    stp  x18, x19, [sp, #144]
    stp  x20, x21, [sp, #160]
    stp  x22, x23, [sp, #176]
    stp  x24, x25, [sp, #192]
    stp  x26, x27, [sp, #208]
    stp  x28, x29, [sp, #224]
    str  x30,       [sp, #240]
    mrs  x9,  elr_el2
    mrs  x10, spsr_el2
    mrs  x11, far_el2
    mrs  x12, esr_el2
    stp  x9,  x10, [sp, #248]
    stp  x11, x12, [sp, #264]
    mov  x0,  sp
    bl   vi_aarch64_trap_handler
    ldp  x9,  x10, [sp, #248]
    msr  elr_el2,  x9
    msr  spsr_el2, x10
    ldp  x0,  x1,  [sp, #0]
    ldp  x2,  x3,  [sp, #16]
    ldp  x4,  x5,  [sp, #32]
    ldp  x6,  x7,  [sp, #48]
    ldp  x8,  x9,  [sp, #64]
    ldp  x10, x11, [sp, #80]
    ldp  x12, x13, [sp, #96]
    ldp  x14, x15, [sp, #112]
    ldp  x16, x17, [sp, #128]
    ldp  x18, x19, [sp, #144]
    ldp  x20, x21, [sp, #160]
    ldp  x22, x23, [sp, #176]
    ldp  x24, x25, [sp, #192]
    ldp  x26, x27, [sp, #208]
    ldp  x28, x29, [sp, #224]
    ldr  x30,       [sp, #240]
    add  sp, sp, #(35 * 8)
    eret

    // ── Lower-EL sync: Cell SVC or guest EL1 trap ────────────────────────────
    .balign 4
vt_sync_el2_lower:
    // Temporarily save x0 and x1 to the host stack (16-byte aligned scratch area).
    sub  sp, sp, #16
    stp  x0, x1, [sp]
    // Check TPIDR_EL2: non-zero means a vCPU is running and this is a guest trap.
    // SAFETY: TPIDR_EL2 is EL2-private; always 0 when no guest runs.
    mrs  x0, tpidr_el2
    cbnz x0, vt_vcpu_trap      // → vcpu.rs global_asm! handler
    // Guest not running: restore scratch and fall through to the host SVC handler.
    ldp  x0, x1, [sp]
    add  sp, sp, #16
    b    vt_sync_el2_cur

    .balign 4
vt_irq_el2_cur:
vt_irq_el2_lower:
    sub  sp, sp, #(35 * 8)
    stp  x0,  x1,  [sp, #0]
    stp  x2,  x3,  [sp, #16]
    stp  x4,  x5,  [sp, #32]
    stp  x6,  x7,  [sp, #48]
    stp  x8,  x9,  [sp, #64]
    stp  x10, x11, [sp, #80]
    stp  x12, x13, [sp, #96]
    stp  x14, x15, [sp, #112]
    stp  x16, x17, [sp, #128]
    stp  x18, x19, [sp, #144]
    stp  x20, x21, [sp, #160]
    stp  x22, x23, [sp, #176]
    stp  x24, x25, [sp, #192]
    stp  x26, x27, [sp, #208]
    stp  x28, x29, [sp, #224]
    str  x30,       [sp, #240]
    mrs  x9,  elr_el2
    mrs  x10, spsr_el2
    mrs  x11, far_el2
    mrs  x12, esr_el2
    stp  x9,  x10, [sp, #248]
    stp  x11, x12, [sp, #264]
    mov  x0,  sp
    bl   vi_aarch64_irq_handler
    ldp  x9,  x10, [sp, #248]
    msr  elr_el2,  x9
    msr  spsr_el2, x10
    ldp  x0,  x1,  [sp, #0]
    ldp  x2,  x3,  [sp, #16]
    ldp  x4,  x5,  [sp, #32]
    ldp  x6,  x7,  [sp, #48]
    ldp  x8,  x9,  [sp, #64]
    ldp  x10, x11, [sp, #80]
    ldp  x12, x13, [sp, #96]
    ldp  x14, x15, [sp, #112]
    ldp  x16, x17, [sp, #128]
    ldp  x18, x19, [sp, #144]
    ldp  x20, x21, [sp, #160]
    ldp  x22, x23, [sp, #176]
    ldp  x24, x25, [sp, #192]
    ldp  x26, x27, [sp, #208]
    ldp  x28, x29, [sp, #224]
    ldr  x30,       [sp, #240]
    add  sp, sp, #(35 * 8)
    eret
"#);
