use super::*;

const GGTT: u32 = 0x0090_0000;

fn regs(image: &[u32]) -> &[u32] {
    &image[REGS_OFFSET as usize / 4..WA_BB_OFFSET as usize / 4]
}

#[test]
fn xe2_render_offsets_decode_to_the_upstream_lri_lists() {
    let (image, _) = render_lrc(&LrcParams {
        ggtt: GGTT,
        pdp: 0,
        asid: 0,
        initial_seqno: 0,
    });
    let r = regs(&image);
    let lri = |count: u32, posted: bool| {
        MI_LOAD_REGISTER_IMM
            | mi_lri_num_regs(count)
            | MI_LRI_LRM_CS_MMIO
            | if posted { MI_LRI_FORCE_POSTED } else { 0 }
    };
    assert_eq!(r[0], 0); // NOP(1)
    assert_eq!(r[1], 0x1108_101d);
    assert_eq!(r[1], lri(15, true));
    let first: [u32; 15] = [
        0x244, 0x034, 0x030, 0x038, 0x03c, 0x168, 0x140, 0x110, 0x1c0, 0x1c4, 0x1c8, 0x180, 0x2b4,
        0x120, 0x124,
    ];
    for (i, off) in first.iter().enumerate() {
        assert_eq!(r[2 + 2 * i], 0x2000 + off, "first list reg {i}");
    }
    assert_eq!(r[0x21], lri(9, true));
    let second: [u32; 9] = [
        0x3a8, 0x3ac, 0x108, 0x284, 0x280, 0x27c, 0x278, 0x274, 0x270,
    ];
    for (i, off) in second.iter().enumerate() {
        assert_eq!(r[0x22 + 2 * i], 0x2000 + off, "second list reg {i}");
    }
    assert_eq!(r[0x36], lri(2, true));
    assert_eq!((r[0x37], r[0x39]), (0x25a8, 0x25ac));
    assert_eq!(r[0x41], lri(1, false));
    assert_eq!(r[0x42], 0x20c8);
    assert_eq!(r[0x44], MI_BATCH_BUFFER_END | 1);
    // The CTX_* value slots sit right after their register addresses.
    assert_eq!(r[CTX_RING_TAIL - 1], 0x2030);
    assert_eq!(r[CTX_PDP0_LDW - 1], 0x2270);
    assert_eq!(r[CTX_ASID - 1], 0x2278);
}

#[test]
fn render_lrc_values_follow_xe_lrc_ctx_init() {
    let pdp = 0x1_2345_6000 | 0b11 | 1 << 4; // present, RW, WB PAT bits
    let (image, tail) = render_lrc(&LrcParams {
        ggtt: GGTT,
        pdp,
        asid: 7,
        initial_seqno: 41,
    });
    let r = regs(&image);
    assert_eq!(r[CTX_CONTEXT_CONTROL], 0x0009_0009);
    assert_eq!(r[CTX_RING_START], GGTT);
    assert_eq!((r[CTX_RING_HEAD], r[CTX_RING_TAIL]), (0, 0));
    assert_eq!(r[CTX_RING_CTL], 0x3001);
    assert_eq!(r[CTX_PDP0_UDW], 1);
    assert_eq!(r[CTX_PDP0_LDW], 0x2345_6013);
    assert_eq!(r[CTX_ASID], 7);
    assert_eq!(r[CTX_BB_PER_CTX_PTR], GGTT + WA_BB_OFFSET + 1);
    assert_eq!(image[WA_BB_OFFSET as usize / 4], MI_BATCH_BUFFER_END);
    assert_eq!(image[SEQNO_OFFSET as usize / 4], 41);
    assert_eq!(image[START_SEQNO_OFFSET as usize / 4], 41);
    assert_eq!(&image[..3], &[MI_ARB_ON_OFF | MI_ARB_ENABLE, MI_NOOP, 0]);
    assert_eq!(tail, 8);
    assert_eq!(image.len() * 4, 40 * 1024);
    // PPHWSP left zeroed.
    assert!(image[PPHWSP_OFFSET as usize / 4..REGS_OFFSET as usize / 4]
        .iter()
        .all(|&w| w == 0));
}

#[test]
fn descriptor_points_at_the_pphwsp_with_ppgtt_mode() {
    assert_eq!(descriptor(GGTT), 0x119 | u64::from(GGTT + 16 * 1024));
}

#[test]
fn job_dwords_are_exact() {
    let words = job(GGTT, 0x0001_2345_6780_0000, 9);
    assert_eq!(
        words,
        [
            0x1040_0002,
            GGTT + START_SEQNO_OFFSET,
            0,
            9,
            0x1880_0101,
            0x6780_0000,
            0x0001_2345,
            0x0400_0000,
            0x1040_0002,
            GGTT + SEQNO_OFFSET,
            0,
            9,
            0x0100_0000,
            0x0400_0001,
            0x0280_0000,
            0,
        ]
    );
}

#[test]
fn ring_space_matches_xe_lrc_ring_space() {
    assert_eq!(ring_space(0, 0), RING_SIZE);
    assert_eq!(ring_space(0, 8), RING_SIZE - 8);
    assert_eq!(ring_space(64, 8), 56);
    assert_eq!(ring_space(8, 16), RING_SIZE - 8);
}
