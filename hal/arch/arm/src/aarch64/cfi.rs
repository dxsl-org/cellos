//! AArch64 BTI + PAC-RET Control-Flow Integrity.
//!
//! BTI (Branch Target Identification, ARMv8.5) protects forward edges:
//! every indirect branch must land on a `BTI c` instruction. The compiler
//! emits a BTI landing pad (GNU note `.note.gnu.property` with BTI feature
//! bit) when `-C target-feature=+bti` is passed; the hardware enforces it
//! once BT0/BT1 are set in SCTLR_EL1.
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
//! - `SCTLR_EL1[35]` — BT0: BTI enforcement at EL0
//! - `SCTLR_EL1[36]` — BT1: BTI enforcement at EL1

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
        super::uart_pl011::puts("[INFO] CFI: BTI enabled\n");
    } else {
        super::uart_pl011::puts("[INFO] CFI: BTI unavailable\n");
    }

    if caps.pac {
        // Load a fixed 128-bit dev key (lo=bits[63:0], hi=bits[127:64]).
        // Any non-zero value arms PAC signing; the actual value only matters
        // for offline signature verification (unused at this stage).
        const PAC_KEY_LO: u64 = 0x0123_4567_89AB_CDEF;
        const PAC_KEY_HI: u64 = 0xFEDC_BA98_7654_3210;

        // SAFETY: writing PAC key registers from EL1 is a standard key-setup
        // operation. No pointer or memory invariant is affected.
        unsafe {
            core::arch::asm!(
                "msr apiakeylo_el1, {}",
                in(reg) PAC_KEY_LO,
                options(nomem, nostack)
            );
            core::arch::asm!(
                "msr apiakeyhi_el1, {}",
                in(reg) PAC_KEY_HI,
                options(nomem, nostack)
            );
            core::arch::asm!("isb", options(nomem, nostack));
        }
        super::uart_pl011::puts("[INFO] CFI: PAC-RET enabled (dev key)\n");
    } else {
        super::uart_pl011::puts("[INFO] CFI: PAC-RET unavailable\n");
    }
}
