//! EL2 left behind when firmware enters QEMU `virt` at EL2 on a CPU without
//! FEAT_VHE. The host cannot stay at EL2 there (see
//! [`HostRegime`](super::backend::HostRegime)), so `boot.rs` `.el2_init` drops
//! it to EL1 and points `VBAR_EL2` here: no hypervisor, no Stage-2.
//!
//! An HVC from the EL1 host — the PSCI conduit the kernel infers when no
//! device tree names one — answers SMCCC `NOT_SUPPORTED` instead of jumping
//! through a reset-UNKNOWN `VBAR_EL2`, so the kernel stays single-hart with a
//! warning. Nothing else is routed to EL2 under `HCR_EL2 = RW`; any other
//! exception parks the core.

use core::arch::global_asm;
use core::sync::atomic::{AtomicBool, Ordering};

use super::backend::{ArmPlatform, Board};

/// Set once by the boot core before `kmain` when it left EL2 for this stub.
static EL1_FALLBACK: AtomicBool = AtomicBool::new(false);

/// Called from `boot.rs` `.el1_entry` (after BSS clear, before `kmain`) when
/// the boot core dropped to EL1 because the CPU lacks FEAT_VHE.
#[no_mangle]
pub extern "C" fn el2_mark_el1_fallback() {
    EL1_FALLBACK.store(true, Ordering::Relaxed);
}

/// Name the fallback on the console; paging activation calls this once,
/// before any Cell exists.
pub fn report() {
    if EL1_FALLBACK.load(Ordering::Relaxed) {
        Board::early_puts(
            "[INFO] host regime: EL1 (CPU lacks FEAT_VHE; EL2 holds only the SMCCC stub)\n",
        );
    }
}

global_asm!(
    r#"
    .section .text.vectors
    .balign 2048
    .global __el2_stub_vectors
__el2_stub_vectors:
    // Current EL with SP_EL0 / SP_ELx: EL2 itself never runs code here.
    .rept 8
    .balign 0x80
    b    .Lel2_stub_park
    .endr
    // Lower EL, AArch64, synchronous. Only x0 is touched: it carries the
    // SMCCC result. ELR_EL2 already points past the HVC.
    .balign 0x80
    mrs  x0, esr_el2
    ubfx x0, x0, #26, #6
    cmp  x0, #0x16               // EC = HVC from AArch64
    b.ne .Lel2_stub_park
    mov  x0, #-1                 // SMCCC NOT_SUPPORTED
    eret
    // Lower EL IRQ/FIQ/SError and all lower-EL AArch32 entries.
    .rept 7
    .balign 0x80
    b    .Lel2_stub_park
    .endr
.Lel2_stub_park:
    msr  daifset, #0xf
    wfi
    b    .Lel2_stub_park
    .section .text
    "#
);
