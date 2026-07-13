//! AArch64 MTE (Memory Tagging Extension) support.
//!
//! ⚠️  MTE is HARDENING ONLY — 1/16 probabilistic tag collision, and
//! TikTag (2024) shows speculative bypass via cache-timing side channels.
//! It detects use-after-free and linear overflows; it does NOT prevent a
//! deliberate, knowledge-bearing attacker from bypassing checks.
//!
//! ## SCTLR_EL1 fields used (DDI0487 §D17.2.118)
//! - `ATA`  [43] — allow tag load/store at EL1
//! - `ATA0` [42] — allow tag load/store at EL0
//! - `TCF`  [41:40] — EL1 tag check fault mode: `0b10` = sync, `0b01` = async
//! - `TCF0` [39:38] — EL0 tag check fault mode: same encoding
//!
//! ## ID_AA64PFR1_EL1[11:8] — MTE field
//! - 0b0000 = not implemented
//! - 0b0001 = MTE1 (store-only tagging, no load check)
//! - 0b0010 = MTE2 (full load+store tag checking)
//! - 0b0011 = MTE3 (+ async mode w/ precise tagging option)
//!
//! Value ≥ 2 is required for useful `TCF` enforcement.

use hal_mte::ViMte;

/// AArch64 MTE2 implementation of `ViMte`.
pub struct AArch64Mte;

impl ViMte for AArch64Mte {
    fn is_available() -> bool {
        let pfr1: u64;
        // SAFETY: mrs from a read-only ID register; no memory side effects.
        unsafe {
            core::arch::asm!(
                "mrs {}, id_aa64pfr1_el1",
                out(reg) pfr1,
                options(nomem, nostack)
            );
        }
        // Bits [11:8] are the MTE field; ≥ 2 = MTE2 (full checking available).
        ((pfr1 >> 8) & 0xF) >= 2
    }

    unsafe fn tag_region(ptr: *mut u8, len: usize, color: u8) {
        // Build a Tagged Pointer: bits [59:56] carry the 4-bit allocation tag.
        // The address range bits and TBI (Top Byte Ignore) are already stripped.
        // STG stores the tag at the granule aligned to `ptr`; it does NOT write
        // data — only the tag memory (out-of-band 4-bit storage) is updated.
        //
        // SAFETY: caller guarantees `ptr` is 16-byte aligned, `len` is a
        // non-zero multiple of 16, and the range is valid Normal-Tagged memory
        // owned exclusively by the caller.
        let tag = (color & 0xF) as u64;
        let mut cur = ptr as u64;
        let end = cur + len as u64;
        while cur < end {
            // Clear existing tag bits [59:56] and insert new tag.
            let tagged_ptr = (cur & !(0xFu64 << 56)) | (tag << 56);
            // SAFETY: `tagged_ptr` points into the caller-guaranteed valid region;
            // STG only touches tag memory for the 16-byte granule at this address.
            unsafe {
                core::arch::asm!(
                    "stg {p}, [{p}]",
                    p = in(reg) tagged_ptr,
                    options(nostack)
                );
            }
            cur += 16;
        }
    }

    unsafe fn get_tag(ptr: *const u8) -> u8 {
        // LDG loads the allocation tag for the granule at `ptr` into bits [59:56]
        // of the output register; all other bits are copied from the input pointer.
        //
        // SAFETY: caller guarantees `ptr` is inside a valid Normal-Tagged region.
        let mut result: u64 = ptr as u64;
        unsafe {
            core::arch::asm!(
                "ldg {r}, [{r}]",
                r = inout(reg) result,
                options(nostack)
            );
        }
        // Extract bits [59:56].
        ((result >> 56) & 0xF) as u8
    }

    fn set_check_mode(synchronous: bool) {
        // RMW SCTLR_EL1: clear TCF[41:40] and TCF0[39:38], then set chosen mode.
        // 0b10 = synchronous fault, 0b01 = asynchronous fault.
        let sctlr: u64;
        // SAFETY: reading SCTLR_EL1 from EL1 has no side effects.
        unsafe {
            core::arch::asm!(
                "mrs {}, sctlr_el1",
                out(reg) sctlr,
                options(nomem, nostack)
            );
        }
        let mode: u64 = if synchronous { 0b10 } else { 0b01 };
        // Mask out bits [41:38] then set TCF and TCF0 to the chosen mode.
        let new_sctlr = (sctlr & !(0xFu64 << 38)) | (mode << 40) | (mode << 38);
        // SAFETY: TCF/TCF0 control how tag faults are reported; no memory
        // safety invariant is affected by changing fault-delivery mode.
        unsafe {
            core::arch::asm!(
                "msr sctlr_el1, {}",
                in(reg) new_sctlr,
                options(nomem, nostack)
            );
            core::arch::asm!("isb", options(nomem, nostack));
        }
    }
}

/// Enable MTE if the hardware supports MTE2. Called from `AArch64Arch::init()`.
///
/// Sets ATA, ATA0, TCF=sync, TCF0=sync in SCTLR_EL1 and emits an ISB.
/// Logs the outcome via the PL011 UART.
pub fn init() {
    if !AArch64Mte::is_available() {
        super::uart_pl011::puts("[INFO] MTE: unavailable\n");
        return;
    }

    let sctlr: u64;
    // SAFETY: reading SCTLR_EL1 from EL1; no memory side effects.
    unsafe {
        core::arch::asm!(
            "mrs {}, sctlr_el1",
            out(reg) sctlr,
            options(nomem, nostack)
        );
    }

    let new_sctlr = sctlr
        | (1u64 << 43)      // ATA:  allow tag access at EL1
        | (1u64 << 42)      // ATA0: allow tag access at EL0
        | (0b10u64 << 40)   // TCF:  synchronous fault at EL1
        | (0b10u64 << 38);  // TCF0: synchronous fault at EL0

    // SAFETY: SCTLR_EL1 write from EL1 to set MTE control bits.
    // ATA/ATA0 enable tag-memory access; TCF/TCF0 choose fault delivery.
    // Neither bit affects the validity of existing pointers or mappings.
    unsafe {
        core::arch::asm!(
            "msr sctlr_el1, {}",
            in(reg) new_sctlr,
            options(nomem, nostack)
        );
        core::arch::asm!("isb", options(nomem, nostack));
    }

    super::uart_pl011::puts("[INFO] MTE: enabled (sync fault)\n");
}
