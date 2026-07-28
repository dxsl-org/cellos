//! ID_AA64* register emulation for `HCR_EL2.TID3`-trapped guest MRS reads.
//!
//! `TID3` traps the ENTIRE AArch64 "ID register group 3" (ARM DDI 0487
//! D23.1: Op0=3, Op1=0, CRn=0, CRm=1..7) to EL2, not just the one encoding a
//! given guest kernel happens to probe first. Letting every trapped read
//! RAZ (the pre-existing `ViVmExit::SysReg` fallback) would hide real CPU
//! feature bits from the guest's cpufeature detection and send it down the
//! wrong code paths later in boot — so every defined encoding in the group
//! is passed through to the real host value here; only genuinely reserved
//! encodings fall back to the caller's RAZ default.

/// Read the AArch64 system register at raw encoding `S3_0_C0_C<crm>_<op2>`.
///
/// Uses the generic `Sop0_op1_Cn_Cm_op2` mnemonic (not the named aliases
/// like `id_aa64zfr0_el1`) because several ID registers in this group were
/// added by later architecture revisions than this toolchain's assembler
/// register table recognizes by name; the raw encoding always assembles.
///
/// # Safety
/// `$crm`/`$op2` must select one of the ID registers enumerated in
/// [`read_trapped_id_reg`] — all are unconditionally readable from EL2 with
/// no side effects.
macro_rules! read_id {
    ($crm:literal, $op2:literal) => {{
        let val: u64;
        core::arch::asm!(
            concat!("mrs {0}, S3_0_C0_C", $crm, "_", $op2),
            out(reg) val,
            options(nomem, nostack),
        );
        val
    }};
}

/// Real host value for a guest MRS of a trapped ID_AA64* register, or `None`
/// if the encoding is outside AArch64 ID group 3, or is a reserved slot
/// within it (the caller should RAZ those, matching the architectural
/// convention for unallocated ID-register space).
///
/// `ID_AA64MMFR0_EL1.PARange` (bits[3:0]) is clamped to `0b0010` (40-bit) so
/// the guest can never program `TCR_EL1.IPS` wider than the 40-bit Stage-2
/// IPA space it actually runs inside (`VTCR_EL2.PS` = 40-bit; see
/// `stage2_regs.rs`). Without the clamp the guest sees the host's real,
/// wider PARange, configures a wider physical/IPA size than Stage-2 can
/// translate, and its own address-size checks later fault against that
/// narrower limit.
pub fn read_trapped_id_reg(op0: u8, op1: u8, crn: u8, crm: u8, op2: u8) -> Option<u64> {
    if op0 != 3 || op1 != 0 || crn != 0 || !(1..=7).contains(&crm) {
        return None;
    }

    // SAFETY: every (crm, op2) pair matched below is a defined ID register —
    // readable from EL2 unconditionally, no side effects — exactly the
    // trap-only group `HCR_EL2.TID3` routes here.
    let raw = unsafe {
        match (crm, op2) {
            // CRm=1: AArch32 processor/debug/aux/memory-model feature IDs.
            (1, 0) => read_id!(1, 0), // ID_PFR0_EL1
            (1, 1) => read_id!(1, 1), // ID_PFR1_EL1
            (1, 2) => read_id!(1, 2), // ID_DFR0_EL1
            (1, 3) => read_id!(1, 3), // ID_AFR0_EL1
            (1, 4) => read_id!(1, 4), // ID_MMFR0_EL1
            (1, 5) => read_id!(1, 5), // ID_MMFR1_EL1
            (1, 6) => read_id!(1, 6), // ID_MMFR2_EL1
            (1, 7) => read_id!(1, 7), // ID_MMFR3_EL1
            // CRm=2: AArch32 instruction-set feature IDs.
            (2, 0) => read_id!(2, 0), // ID_ISAR0_EL1
            (2, 1) => read_id!(2, 1), // ID_ISAR1_EL1
            (2, 2) => read_id!(2, 2), // ID_ISAR2_EL1
            (2, 3) => read_id!(2, 3), // ID_ISAR3_EL1
            (2, 4) => read_id!(2, 4), // ID_ISAR4_EL1
            (2, 5) => read_id!(2, 5), // ID_ISAR5_EL1
            (2, 6) => read_id!(2, 6), // ID_MMFR4_EL1
            (2, 7) => read_id!(2, 7), // ID_ISAR6_EL1
            // CRm=3: AArch32 media/VFP + remaining feature IDs.
            (3, 0) => read_id!(3, 0), // MVFR0_EL1
            (3, 1) => read_id!(3, 1), // MVFR1_EL1
            (3, 2) => read_id!(3, 2), // MVFR2_EL1
            (3, 4) => read_id!(3, 4), // ID_PFR2_EL1
            (3, 6) => read_id!(3, 6), // ID_MMFR5_EL1
            // CRm=4: AArch64 processor feature IDs.
            (4, 0) => read_id!(4, 0), // ID_AA64PFR0_EL1
            (4, 1) => read_id!(4, 1), // ID_AA64PFR1_EL1
            (4, 4) => read_id!(4, 4), // ID_AA64ZFR0_EL1
            // CRm=5: AArch64 debug + auxiliary feature IDs.
            (5, 0) => read_id!(5, 0), // ID_AA64DFR0_EL1
            (5, 1) => read_id!(5, 1), // ID_AA64DFR1_EL1
            (5, 4) => read_id!(5, 4), // ID_AA64AFR0_EL1
            (5, 5) => read_id!(5, 5), // ID_AA64AFR1_EL1
            // CRm=6: AArch64 instruction-set feature IDs.
            (6, 0) => read_id!(6, 0), // ID_AA64ISAR0_EL1
            (6, 1) => read_id!(6, 1), // ID_AA64ISAR1_EL1
            // CRm=7: AArch64 memory-model feature IDs.
            (7, 0) => read_id!(7, 0), // ID_AA64MMFR0_EL1 — PARange clamped below
            (7, 1) => read_id!(7, 1), // ID_AA64MMFR1_EL1
            (7, 2) => read_id!(7, 2), // ID_AA64MMFR2_EL1
            _ => return None,
        }
    };

    if crm == 7 && op2 == 0 {
        const PARANGE_MASK: u64 = 0xF;
        const PARANGE_40BIT: u64 = 0b0010;
        Some((raw & !PARANGE_MASK) | PARANGE_40BIT)
    } else {
        Some(raw)
    }
}
