use super::*;

#[test]
fn headers_match_the_abi_bit_layout() {
    assert_eq!(ct_header(0xbeef, 3), 0xbeef_0003);
    // REQUEST type 0, action 0x1234 with DATA0 0x56 in bits 27:16.
    assert_eq!(hxg_header(HxgType::Request, 0x0056_1234), 0x0056_1234);
    assert_eq!(hxg_header(HxgType::FastRequest, 0x5001), 0x2000_5001);
    // Bits above 27 of the action word never leak into TYPE.
    assert_eq!(hxg_header(HxgType::Request, 0xf000_0001) >> 28, 0);
}

#[test]
fn guc_responses_decode() {
    let ok = decode_hxg(0xf000_0007).unwrap();
    assert_eq!(
        ok,
        Hxg {
            from_guc: true,
            kind: HxgType::ResponseSuccess,
            aux: 7
        }
    );
    let fail = decode_hxg(0xe012_0034).unwrap();
    assert_eq!(
        (fail.kind, fail.aux >> 16, fail.aux & 0xffff),
        (HxgType::ResponseFailure, 0x12, 0x34)
    );
    assert_eq!(decode_hxg(0x4000_0000), None); // type 4 is undefined
}

#[test]
fn space_leaves_one_slot_empty() {
    assert_eq!(space(16, 0, 0), 15);
    assert_eq!(space(16, 5, 4), 0);
    assert_eq!(space(16, 2, 14), 3);
}

#[test]
fn write_places_ct_header_hxg_header_and_data() {
    let mut ring = [0u32; 16];
    let tail = h2g_write(&mut ring, 0, 0, 7, HxgType::Request, &[0x5503, 0xa, 0xb]).unwrap();
    assert_eq!(tail, 4);
    assert_eq!(&ring[..4], &[ct_header(7, 3), 0x5503, 0xa, 0xb]);
}

#[test]
fn write_refuses_when_the_guc_has_not_consumed_enough() {
    let mut ring = [0u32; 16];
    // head 4, tail 0: 3 free dwords, message needs 4.
    assert_eq!(
        h2g_write(&mut ring, 4, 0, 1, HxgType::Request, &[1, 2, 3]),
        Err(CtError::NoSpace)
    );
    assert!(h2g_write(&mut ring, 4, 0, 1, HxgType::Request, &[1, 2]).is_ok());
}

#[test]
fn write_across_the_end_zero_fills_and_asks_for_retry() {
    let mut ring = [0xffff_ffffu32; 16];
    // tail 14, head 10: space 11, but 4 dwords do not fit before the end.
    let err = h2g_write(&mut ring, 10, 14, 1, HxgType::Request, &[1, 2, 3]);
    assert_eq!(err, Err(CtError::Wrapped { new_tail: 0 }));
    assert_eq!(&ring[14..], &[0, 0]);
    assert_eq!(ring[13], 0xffff_ffff);
    assert_eq!(
        h2g_write(&mut ring, 10, 0, 1, HxgType::Request, &[1, 2, 3]),
        Ok(4)
    );
}

#[test]
fn write_validates_length_and_offsets() {
    let mut ring = [0u32; 16];
    assert_eq!(
        h2g_write(&mut ring, 0, 0, 1, HxgType::Request, &[]),
        Err(CtError::BadLength)
    );
    let long = [0u32; MAX_MSG_DWORDS + 1];
    let mut big = vec![0u32; 1024];
    assert_eq!(
        h2g_write(&mut big, 0, 0, 1, HxgType::Request, &long),
        Err(CtError::BadLength)
    );
    assert_eq!(
        h2g_write(&mut ring, 16, 0, 1, HxgType::Request, &[1]),
        Err(CtError::Overflow)
    );
}

#[test]
fn read_returns_payload_and_wraps_around_the_end() {
    let mut ring = [0u32; 16];
    ring[14] = ct_header(9, 3);
    ring[15] = 0xf000_0000;
    ring[0] = 0x11;
    ring[1] = 0x22;
    let mut out = [0u32; MAX_MSG_DWORDS];
    let (fence, len, head) = g2h_read(&ring, 14, 2, &mut out).unwrap().unwrap();
    assert_eq!((fence, len, head), (9, 3, 2));
    assert_eq!(&out[..3], &[0xf000_0000, 0x11, 0x22]);
    assert_eq!(g2h_read(&ring, 2, 2, &mut out), Ok(None));
}

#[test]
fn read_detects_truncated_and_foreign_format_messages() {
    let mut ring = [0u32; 16];
    let mut out = [0u32; MAX_MSG_DWORDS];
    ring[0] = ct_header(1, 5);
    assert_eq!(g2h_read(&ring, 0, 3, &mut out), Err(CtError::Underflow));
    ring[0] = 1 << 12 | 1;
    assert_eq!(g2h_read(&ring, 0, 3, &mut out), Err(CtError::BadFormat));
    assert_eq!(g2h_read(&ring, 0, 16, &mut out), Err(CtError::Overflow));
}

#[test]
fn ring_sizes_are_powers_of_two() {
    assert!(H2G_DWORDS.is_power_of_two() && G2H_DWORDS.is_power_of_two());
}
