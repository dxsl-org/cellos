mod common;

use common::{
    inquiry_data, lun, ready_tail, report_luns_data, FakeError, FakeExecutor, Reply, CONFIG,
};
use scsi_block::cdb::{OP_INQUIRY, OP_REPORT_LUNS, OP_SERVICE_ACTION_IN_16, OP_TEST_UNIT_READY};
use scsi_block::{sense_key, status, ScsiDisk, ScsiError, LUN0, MAX_REPORTED_LUNS};

#[test]
fn picks_first_disk_lun_skipping_non_disk() {
    let mut fake = FakeExecutor::new([
        Reply::data(&report_luns_data(&[lun(0), lun(1), lun(2)])),
        Reply::data(&inquiry_data(0, 5)), // LUN 0: CD-ROM
        Reply::data(&inquiry_data(0, 0)), // LUN 1: disk
    ]);
    fake.script.extend(ready_tail(1000, 512));
    let disk = ScsiDisk::probe(fake, 2, CONFIG).unwrap();
    assert_eq!(disk.lun(), lun(1));
    assert_eq!(disk.target(), 2);
    assert_eq!(disk.block_count(), 1000);
    assert_eq!(disk.block_size(), 512);
    assert_eq!(disk.max_transfer_blocks(), 128);
    assert_eq!(disk.other_luns().collect::<Vec<_>>(), vec![lun(0), lun(2)]);
    assert!(!disk.luns_truncated());

    let fake = disk.into_executor();
    assert_eq!(
        fake.opcodes(),
        vec![
            OP_REPORT_LUNS,
            OP_INQUIRY,
            OP_INQUIRY,
            OP_TEST_UNIT_READY,
            OP_SERVICE_ACTION_IN_16
        ]
    );
    let luns: Vec<_> = fake.calls.iter().map(|call| call.lun).collect();
    assert_eq!(luns, vec![LUN0, lun(0), lun(1), lun(1), lun(1)]);
    assert!(fake.calls.iter().all(|call| call.target == 2));
}

#[test]
fn report_luns_illegal_request_falls_back_to_lun0() {
    let mut fake = FakeExecutor::new([
        Reply::check(sense_key::ILLEGAL_REQUEST, 0x20, 0x00),
        Reply::data(&inquiry_data(0, 0)),
    ]);
    fake.script.extend(ready_tail(16, 4096));
    let disk = ScsiDisk::probe(fake, 0, CONFIG).unwrap();
    assert_eq!(disk.lun(), LUN0);
    assert_eq!(disk.other_luns().count(), 0);
    assert_eq!(disk.into_executor().calls[1].lun, LUN0);
}

#[test]
fn empty_report_luns_falls_back_to_lun0() {
    let mut fake = FakeExecutor::new([
        Reply::data(&report_luns_data(&[])),
        Reply::data(&inquiry_data(0, 0)),
    ]);
    fake.script.extend(ready_tail(16, 512));
    assert_eq!(ScsiDisk::probe(fake, 0, CONFIG).unwrap().lun(), LUN0);
}

#[test]
fn report_luns_other_failure_propagates() {
    let fake = FakeExecutor::new([Reply::status(status::BUSY)]);
    assert_eq!(
        ScsiDisk::probe(fake, 0, CONFIG).err(),
        Some(ScsiError::Status(status::BUSY))
    );
}

#[test]
fn lun_list_truncated_at_capacity() {
    let luns: Vec<_> = (0..=MAX_REPORTED_LUNS as u8).map(lun).collect();
    let mut fake = FakeExecutor::new([
        Reply::data(&report_luns_data(&luns)),
        Reply::data(&inquiry_data(0, 0)),
    ]);
    fake.script.extend(ready_tail(16, 512));
    let disk = ScsiDisk::probe(fake, 0, CONFIG).unwrap();
    assert!(disk.luns_truncated());
    assert_eq!(disk.other_luns().count(), MAX_REPORTED_LUNS - 1);
}

#[test]
fn failing_inquiry_lun_is_skipped() {
    let mut fake = FakeExecutor::new([
        Reply::data(&report_luns_data(&[lun(0), lun(1), lun(2)])),
        Reply::check(sense_key::HARDWARE_ERROR, 0x44, 0x00),
        Reply::data(&[0, 0]), // truncated INQUIRY
        Reply::data(&inquiry_data(0, 0)),
    ]);
    fake.script.extend(ready_tail(16, 512));
    assert_eq!(ScsiDisk::probe(fake, 0, CONFIG).unwrap().lun(), lun(2));
}

#[test]
fn no_disk_lun() {
    let fake = FakeExecutor::new([
        Reply::data(&report_luns_data(&[lun(0), lun(1)])),
        Reply::data(&inquiry_data(0, 5)),
        Reply::data(&inquiry_data(1, 0)),
    ]);
    assert_eq!(
        ScsiDisk::probe(fake, 0, CONFIG).err(),
        Some(ScsiError::NoDisk)
    );
}

#[test]
fn transport_error_during_inquiry_propagates() {
    let fake = FakeExecutor::new([
        Reply::data(&report_luns_data(&[lun(0)])),
        Reply::transport(),
    ]);
    assert_eq!(
        ScsiDisk::probe(fake, 0, CONFIG).err(),
        Some(ScsiError::Transport(FakeError))
    );
}

#[test]
fn borrowed_executor_survives_failed_probe() {
    let mut fake = FakeExecutor::new([
        Reply::data(&report_luns_data(&[lun(0)])),
        Reply::data(&inquiry_data(0, 5)),
    ]);
    assert_eq!(
        ScsiDisk::probe(&mut fake, 0, CONFIG).err(),
        Some(ScsiError::NoDisk)
    );
    fake.script.extend([
        Reply::data(&report_luns_data(&[lun(0)])),
        Reply::data(&inquiry_data(0, 0)),
    ]);
    fake.script.extend(ready_tail(16, 512));
    let disk = ScsiDisk::probe(&mut fake, 1, CONFIG).unwrap();
    assert_eq!(disk.target(), 1);
}
