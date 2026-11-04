use scsi_block::cdb;

#[test]
fn inquiry_layout() {
    assert_eq!(cdb::inquiry(0x0124), [0x12, 0, 0, 0x01, 0x24, 0]);
}

#[test]
fn test_unit_ready_layout() {
    assert_eq!(cdb::test_unit_ready(), [0; 6]);
}

#[test]
fn report_luns_layout() {
    assert_eq!(
        cdb::report_luns(0x0102_0304),
        [0xA0, 0, 0, 0, 0, 0, 0x01, 0x02, 0x03, 0x04, 0, 0]
    );
}

#[test]
fn read_capacity_16_layout() {
    let mut expected = [0u8; 16];
    expected[0] = 0x9E;
    expected[1] = 0x10;
    expected[13] = 32;
    assert_eq!(cdb::read_capacity_16(), expected);
}

#[test]
fn read_16_layout() {
    assert_eq!(
        cdb::read_16(0x0102_0304_0506_0708, 0x0A0B_0C0D),
        [0x88, 0, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x0A, 0x0B, 0x0C, 0x0D, 0, 0]
    );
}

#[test]
fn write_16_layout() {
    assert_eq!(
        cdb::write_16(u64::MAX, 1),
        [0x8A, 0, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0, 0, 0, 1, 0, 0]
    );
}

#[test]
fn synchronize_cache_10_layout() {
    assert_eq!(
        cdb::synchronize_cache_10(),
        [0x35, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    );
}
