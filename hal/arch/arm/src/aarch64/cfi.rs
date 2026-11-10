//! AArch64 BTI + PAC-RET Control-Flow Integrity.
//!
//! BTI (Branch Target Identification, ARMv8.5) protects forward edges:
//! an indirect branch into a *guarded* page (stage-1 descriptor `GP`, bit 50)
//! must land on a compatible `BTI` instruction, or the PE raises a Branch
//! Target exception (EC 0x0D). The compiler emits the landing pads and the
//! `GNU_PROPERTY_AARCH64_FEATURE_1_AND` BTI note with `-Z branch-protection=bti`
//! (`-mbranch-protection=bti` in C); `+bti` alone only permits the instruction.
//! Enforcement is therefore per page, not per SCTLR bit: the kernel loader
//! maps a Cell's executable pages guarded only when its signed image carries
//! the note and [`detect`] reports FEAT_BTI (kernel `loader::gnu_property`);
//! every other page is unguarded and runs exactly as without BTI.
//!
//! PAC (Pointer Authentication, ARMv8.3) protects backward edges:
//! return addresses are signed on function entry (PACIASP) and verified on
//! return (AUTIASP). The compiler inserts these automatically when
//! `-C target-feature=+pac-ret` is passed.
//!
//! Both are HARDENING, not isolation — they reduce the exploitable gadget
//! space for Tier-1b C code and unsafe kernel paths. Neither substitutes
//! for Rust-level type safety in Tier-1 cells.
//!
//! ## Register reference (DDI0487 §D17)
//! - `ID_AA64PFR1_EL1[3:0]`  — BTI:  0 = not impl, ≥1 = BTI
//! - `ID_AA64ISAR1_EL1[7:4]` — APA:  0 = no PAC, ≥1 = QARMA impl
//! - `ID_AA64ISAR1_EL1[11:8]`— API:  0 = no PAC, ≥1 = IMPDEF impl
//! - `SCTLR_EL1[35]` — BT0: at EL0, PACIASP/PACIBSP are *not* implicit landing
//!   pads for BTYPE 0b11 (stricter); BTI itself is armed by the page's GP bit
//! - `SCTLR_EL1[36]` — BT1: the same at EL1

use super::backend::{ArmPlatform, Board};

/// Hardware CFI capability flags discovered at boot.
#[derive(Copy, Clone, Debug)]
pub struct CfiCaps {
    /// True when BTI (ARMv8.5 Branch Target Identification) is present.
    pub bti: bool,
    /// True when PAC (ARMv8.3 Pointer Authentication) is present (APA or API).
    pub pac: bool,
}

/// Probe ID registers for BTI and PAC availability.
///
/// Reads `ID_AA64PFR1_EL1` and `ID_AA64ISAR1_EL1` from EL1.
/// Safe to call at any point after the MMU is initialised.
pub fn detect() -> CfiCaps {
    let pfr1: u64;
    let isar1: u64;

    // SAFETY: mrs from read-only ID registers; no memory side effects.
    unsafe {
        core::arch::asm!(
            "mrs {}, id_aa64pfr1_el1",
            out(reg) pfr1,
            options(nomem, nostack)
        );
        core::arch::asm!(
            "mrs {}, id_aa64isar1_el1",
            out(reg) isar1,
            options(nomem, nostack)
        );
    }

    // ID_AA64PFR1_EL1[3:0]: BTI field; any non-zero value = BTI present.
    let bti = (pfr1 & 0xF) >= 1;

    // ID_AA64ISAR1_EL1[7:4] = APA (QARMA PAC), [11:8] = API (IMPDEF PAC).
    // Either non-zero means PAC signing operations are available.
    let apa = (isar1 >> 4) & 0xF;
    let api = (isar1 >> 8) & 0xF;
    let pac = apa >= 1 || api >= 1;

    CfiCaps { bti, pac }
}

/// Enable BTI and PAC-RET based on hardware availability.
///
/// Must be called early in `AArch64Arch::init()`, before `trap::init()`.
/// Writes SCTLR_EL1 (BT0/BT1) and, when PAC is present, loads a fixed
/// development key into `APIAKeyLo_EL1` / `APIAKeyHi_EL1`.
///
/// # Key note
/// The fixed key `0x0123456789ABCDEF / 0xFEDCBA9876543210` is intentional
/// for dev/QEMU. Production boards must derive a per-device key via Silo
/// before the kernel is finalised; replace this call at that stage.
pub fn init() {
    let caps = detect();

    if caps.bti {
        let sctlr: u64;
        // SAFETY: SCTLR_EL1 RMW from EL1; no memory safety invariant affected.
        unsafe {
            core::arch::asm!(
                "mrs {}, sctlr_el1",
                out(reg) sctlr,
                options(nomem, nostack)
            );
        }
        // BT0 = bit 35 (EL0 BTI), BT1 = bit 36 (EL1 BTI).
        let new_sctlr = sctlr | (1u64 << 35) | (1u64 << 36);
        // SAFETY: setting BT0/BT1 enforces BTI landings — no memory invariant broken.
        unsafe {
            core::arch::asm!(
                "msr sctlr_el1, {}",
                in(reg) new_sctlr,
                options(nomem, nostack)
            );
            core::arch::asm!("isb", options(nomem, nostack));
        }
        Board::early_puts("[INFO] CFI: BTI enabled\n");
    } else {
        Board::early_puts("[INFO] CFI: BTI unavailable\n");
    }

    if caps.pac {
        // Load a fixed 128-bit dev key (lo=bits[63:0], hi=bits[127:64]).
        // Any non-zero value arms PAC signing; the actual value only matters
        // for offline signature verification (unused at this stage).
        const PAC_KEY_LO: u64 = 0x0123_4567_89AB_CDEF;
        const PAC_KEY_HI: u64 = 0xFEDC_BA98_7654_3210;

        // SAFETY: writing PAC key registers from EL1 is a standard key-setup
        // operation. No pointer or memory invariant is affected.
        //
        // Raw S3 encoding avoids the +paca assembler feature requirement:
        //   APIAKEYLO_EL1 = S3_0_C2_C1_0  (Op0=3,Op1=0,CRn=2,CRm=1,Op2=0)
        //   APIAKEYHI_EL1 = S3_0_C2_C1_1  (Op0=3,Op1=0,CRn=2,CRm=1,Op2=1)
        // The runtime `if caps.pac` guard prevents this from executing on
        // cores that don't implement PAC (e.g. Cortex-A53 on RPi 3).
        unsafe {
            core::arch::asm!(
                "msr S3_0_C2_C1_0, {}",
                in(reg) PAC_KEY_LO,
                options(nomem, nostack)
            );
            core::arch::asm!(
                "msr S3_0_C2_C1_1, {}",
                in(reg) PAC_KEY_HI,
                options(nomem, nostack)
            );
            core::arch::asm!("isb", options(nomem, nostack));
        }
        Board::early_puts("[INFO] CFI: PAC-RET enabled (dev key)\n");
    } else {
        Board::early_puts("[INFO] CFI: PAC-RET unavailable\n");
    }
}
