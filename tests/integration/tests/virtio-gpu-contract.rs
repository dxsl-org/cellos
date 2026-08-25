#[allow(dead_code)]
#[path = "../../../cells/services/hypervisor/src/virtio_gpu/wire.rs"]
mod wire;

fn put_u32(raw: &mut [u8], offset: usize, value: u32) {
    raw[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(raw: &mut [u8], offset: usize, value: u64) {
    raw[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

#[test]
fn display_info_is_full_sized_and_echoes_fence() {
    let header = wire::CtrlHeader {
        command: wire::GET_DISPLAY_INFO,
        flags: 1,
        fence_id: 0x1122_3344_5566_7788,
    };
    let response = wire::display_info(header, 1280, 720);
    assert_eq!(response.len(), 408);
    assert_eq!(
        u32::from_le_bytes(response[0..4].try_into().unwrap()),
        wire::OK_DISPLAY_INFO
    );
    assert_eq!(u32::from_le_bytes(response[4..8].try_into().unwrap()), 1);
    assert_eq!(
        u64::from_le_bytes(response[8..16].try_into().unwrap()),
        header.fence_id
    );
    assert_eq!(
        u32::from_le_bytes(response[32..36].try_into().unwrap()),
        1280
    );
    assert_eq!(
        u32::from_le_bytes(response[36..40].try_into().unwrap()),
        720
    );
    assert_eq!(u32::from_le_bytes(response[40..44].try_into().unwrap()), 1);
}

#[test]
fn attach_backing_rejects_huge_or_truncated_entry_lists() {
    let mut raw = vec![0u8; wire::CTRL_HDR_LEN + 8];
    put_u32(&mut raw, wire::CTRL_HDR_LEN, 7);
    put_u32(&mut raw, wire::CTRL_HDR_LEN + 4, 129);
    assert!(wire::parse_attach_backing(&raw).is_none());

    put_u32(&mut raw, wire::CTRL_HDR_LEN + 4, 1);
    assert!(wire::parse_attach_backing(&raw).is_none());
}

#[test]
fn attach_backing_accepts_the_bounded_maximum() {
    const COUNT: usize = 128;
    let mut raw = vec![0u8; wire::CTRL_HDR_LEN + 8 + COUNT * 16];
    put_u32(&mut raw, wire::CTRL_HDR_LEN, 9);
    put_u32(&mut raw, wire::CTRL_HDR_LEN + 4, COUNT as u32);
    for index in 0..COUNT {
        let offset = wire::CTRL_HDR_LEN + 8 + index * 16;
        put_u64(&mut raw, offset, 0x4000_0000 + (index as u64) * 4096);
        put_u32(&mut raw, offset + 8, 4096);
    }
    let (resource_id, entries) = wire::parse_attach_backing(&raw).unwrap();
    assert_eq!(resource_id, 9);
    assert_eq!(entries.len(), COUNT);
    assert_eq!(entries[COUNT - 1].length, 4096);
}

#[test]
fn cursor_and_resource_codecs_reject_short_messages() {
    let short = [0u8; wire::CTRL_HDR_LEN];
    assert!(wire::parse_create_2d(&short).is_none());
    assert!(wire::parse_cursor(&short, true).is_none());

    let mut cursor = [0u8; wire::CTRL_HDR_LEN + 28];
    put_u32(&mut cursor, wire::CTRL_HDR_LEN, 0);
    put_u32(&mut cursor, wire::CTRL_HDR_LEN + 4, 15);
    put_u32(&mut cursor, wire::CTRL_HDR_LEN + 8, 20);
    put_u32(&mut cursor, wire::CTRL_HDR_LEN + 16, 4);
    put_u32(&mut cursor, wire::CTRL_HDR_LEN + 20, 2);
    put_u32(&mut cursor, wire::CTRL_HDR_LEN + 24, 3);
    let parsed = wire::parse_cursor(&cursor, true).unwrap();
    assert_eq!(parsed.resource_id, 4);
    assert_eq!(
        (parsed.x, parsed.y, parsed.hot_x, parsed.hot_y),
        (15, 20, 2, 3)
    );
}
