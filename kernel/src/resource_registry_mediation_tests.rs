use super::{decode, mediates, user_pages, Access};

/// BCM2837 mailbox window: 0x40 bytes on the legacy IRQ controller's page.
const MAILBOX: usize = 0x3F00_B880;
const MAILBOX_LEN: usize = 0x40;

/// EL0 data abort, permission fault at level 3, no syndrome (ISV = 0): the
/// exact ESR the raspi3b lane recorded for the display cell's mailbox read.
const ESR_LOAD: u64 = 0x9200_000F;
const ESR_STORE: u64 = ESR_LOAD | (1 << 6);
/// `ldr w10, [x24, #0x18]` — the display cell's MAILBOX_READ_STATUS poll.
const LDR_W10_IMM: u32 = 0xB940_1B0A;

fn access(write: bool, size: usize, reg: usize, sign_extend: bool, wide: bool) -> Access {
    Access {
        write,
        size,
        reg,
        sign_extend,
        wide,
    }
}

#[test]
fn decode_reads_plain_loads_and_stores_from_the_instruction() {
    assert_eq!(
        decode(ESR_LOAD, LDR_W10_IMM),
        Some(access(false, 4, 10, false, false))
    );
    // str w9, [x8, #0x20]
    assert_eq!(
        decode(ESR_STORE, 0xB900_2109),
        Some(access(true, 4, 9, false, false))
    );
    // ldrsb x1, [x2, x3]
    assert_eq!(
        decode(ESR_LOAD, 0x38A3_6841),
        Some(access(false, 1, 1, true, true))
    );
    // ldur w0, [x1, #-4]
    assert_eq!(
        decode(ESR_LOAD, 0xB85F_C020),
        Some(access(false, 4, 0, false, false))
    );
    // An ISV syndrome does not change the result.
    assert!(decode(ESR_LOAD | (1 << 24), LDR_W10_IMM).is_some());
}

#[test]
fn decode_refuses_instructions_it_cannot_complete_exactly() {
    for insn in [
        0xB840_4420u32, // ldr w0, [x1], #4 (post-index writeback)
        0xB840_4C20,    // ldr w0, [x1, #4]! (pre-index writeback)
        0x2940_0440,    // ldp w0, w1, [x2]
        0xF980_0000,    // prfm
        0x3DC0_0000,    // ldr q0 (SIMD/FP)
        0xB840_0820,    // ldtr w0, [x1] (unprivileged)
        0xB820_0041,    // ldadd w0, w1, [x2] (atomic)
        0x885F_7C20,    // ldxr w0, [x1] (exclusive)
    ] {
        assert_eq!(decode(ESR_LOAD, insn), None, "insn {insn:#x}");
    }
    // The syndrome's direction must match the instruction's.
    assert_eq!(decode(ESR_STORE, LDR_W10_IMM), None);
    assert_eq!(decode(ESR_LOAD, 0xB900_2109), None);
}

#[test]
fn decode_refuses_faults_the_kernel_must_not_complete() {
    let translation = (ESR_LOAD & !0x3F) | 0x07;
    let kernel_ec = (ESR_LOAD & !(0x3F << 26)) | (0x25 << 26);
    let short_insn = ESR_LOAD & !(1 << 25);
    let cache_maint = ESR_LOAD | (1 << 8);
    let far_invalid = ESR_LOAD | (1 << 10);
    let walk = ESR_LOAD | (1 << 7);
    for esr in [
        translation,
        kernel_ec,
        short_insn,
        cache_maint,
        far_invalid,
        walk,
    ] {
        assert_eq!(decode(esr, LDR_W10_IMM), None, "esr {esr:#x}");
    }
}

#[test]
fn mediation_is_confined_to_granted_bytes() {
    // Mailbox registers the display cell uses.
    for offset in [0x00, 0x18, 0x20, 0x38] {
        assert!(mediates(MAILBOX, MAILBOX_LEN, MAILBOX + offset, 4));
    }
    // IRQ_ENABLE1 / ARM timer on the same page, the end edge, misalignment.
    assert!(!mediates(MAILBOX, MAILBOX_LEN, 0x3F00_B210, 4));
    assert!(!mediates(MAILBOX, MAILBOX_LEN, 0x3F00_B408, 4));
    assert!(!mediates(MAILBOX, MAILBOX_LEN, MAILBOX + MAILBOX_LEN, 4));
    assert!(!mediates(MAILBOX, MAILBOX_LEN, MAILBOX + 0x3E, 4));
    assert!(!mediates(MAILBOX, MAILBOX_LEN, MAILBOX + 2, 4));
    // A whole-page grant is USER-mapped; a fault there is not mediated.
    assert!(!mediates(0x3F20_0000, 0x1000, 0x3F20_0034, 4));
}

#[test]
fn loads_extend_like_the_instruction() {
    let ldrsb_w = Access {
        write: false,
        size: 1,
        reg: 0,
        sign_extend: true,
        wide: false,
    };
    assert_eq!(ldrsb_w.extend(0x80), 0xFFFF_FF80);
    let ldrsh_x = Access {
        size: 2,
        wide: true,
        ..ldrsb_w
    };
    assert_eq!(ldrsh_x.extend(0x8000), 0xFFFF_FFFF_FFFF_8000);
    let ldr_w = Access {
        size: 4,
        sign_extend: false,
        ..ldrsb_w
    };
    assert_eq!(ldr_w.extend(0x8000_0001), 0x8000_0001);
}
