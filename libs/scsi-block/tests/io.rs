mod common;

use common::{capacity_data, inquiry_data, lun, report_luns_data, FakeExecutor, Reply};
use scsi_block::{cdb, sense_key, ProbeConfig, ScsiDisk, ScsiError, Sense};

const BLOCKS: u64 = 100;
const BS: usize = 512;

/// A probed 100 x 512-byte disk with an 8-block transfer limit; `replies`
/// follow the probe sequence.
fn disk(replies: impl IntoIterator<Item = Reply>) -> ScsiDisk<FakeExecutor> {
    let mut fake = FakeExecutor::new([
        Reply::data(&report_luns_data(&[lun(0)])),
        Reply::data(&inquiry_data(0, 0)),
        Reply::good(),
        Reply::data(&capacity_data(BLOCKS, BS as u32)),
    ]);
    fake.script.extend(replies);
    let config = ProbeConfig {
        tur_retries: 0,
        max_transfer_bytes: 8 * BS as u32,
    };
    ScsiDisk::probe(fake, 3, config).unwrap()
}

fn last_call(disk: ScsiDisk<FakeExecutor>) -> common::Call {
    disk.into_executor().calls.pop().unwrap()
}

#[test]
fn read_issues_read_16_and_fills_buffer() {
    let payload: Vec<u8> = (0..2 * BS).map(|i| i as u8).collect();
    let mut disk = disk([Reply::data(&payload)]);
    let mut buf = vec![0u8; 2 * BS];
    disk.read(98, &mut buf).unwrap();
    assert_eq!(buf, payload);
    let call = last_call(disk);
    assert_eq!(call.cdb, cdb::read_16(98, 2));
    assert_eq!((call.target, call.lun, call.data_len), (3, lun(0), 2 * BS));
}

#[test]
fn read_split_issues_one_read_16_per_transfer_limit() {
    // 10 blocks against the 8-block limit: blocks 90..98, then 98..100.
    let payload: Vec<u8> = (0..10 * BS).map(|i| (i / BS) as u8).collect();
    let mut disk = disk([
        Reply::data(&payload[..8 * BS]),
        Reply::data(&payload[8 * BS..]),
    ]);
    let mut buf = vec![0u8; 10 * BS];
    disk.read_split(90, &mut buf).unwrap();
    assert_eq!(buf, payload);
    let calls = disk.into_executor().calls;
    let reads: Vec<_> = calls[calls.len() - 2..]
        .iter()
        .map(|c| c.cdb.clone())
        .collect();
    assert_eq!(reads, [cdb::read_16(90, 8), cdb::read_16(98, 2)]);
}

#[test]
fn write_issues_write_16_with_data() {
    let mut disk = disk([Reply::good()]);
    let buf = vec![0xA5u8; 8 * BS];
    disk.write(0, &buf).unwrap();
    let call = last_call(disk);
    assert_eq!(call.cdb, cdb::write_16(0, 8));
    assert_eq!(call.data_out, buf);
}

#[test]
fn flush_issues_synchronize_cache() {
    let mut disk = disk([Reply::good()]);
    disk.flush().unwrap();
    assert_eq!(last_call(disk).cdb, cdb::synchronize_cache_10());
}

#[test]
fn misaligned_buffers_rejected_without_command() {
    let mut disk = disk([]);
    let mut buf = vec![0u8; BS + 1];
    let misaligned = ScsiError::Misaligned {
        len: BS + 1,
        block_size: BS as u32,
    };
    assert_eq!(disk.read(0, &mut buf), Err(misaligned));
    assert_eq!(disk.write(0, &buf), Err(misaligned));
    assert_eq!(disk.into_executor().calls.len(), 4);
}

#[test]
fn out_of_range_rejected() {
    let mut disk = disk([]);
    let mut buf = vec![0u8; 2 * BS];
    assert_eq!(
        disk.read(99, &mut buf),
        Err(ScsiError::OutOfRange {
            lba: 99,
            blocks: 2,
            block_count: BLOCKS
        })
    );
    assert_eq!(
        disk.write(u64::MAX, &buf),
        Err(ScsiError::OutOfRange {
            lba: u64::MAX,
            blocks: 2,
            block_count: BLOCKS
        })
    );
    assert_eq!(disk.into_executor().calls.len(), 4);
}

#[test]
fn over_transfer_limit_rejected() {
    let mut disk = disk([]);
    let buf = vec![0u8; 9 * BS];
    assert_eq!(
        disk.write(0, &buf),
        Err(ScsiError::TooLarge {
            blocks: 9,
            max_blocks: 8
        })
    );
}

#[test]
fn empty_buffer_issues_no_command() {
    let mut disk = disk([]);
    disk.read(BLOCKS, &mut []).unwrap();
    disk.write(0, &[]).unwrap();
    assert_eq!(disk.into_executor().calls.len(), 4);
}

#[test]
fn short_read_is_an_error() {
    let mut disk = disk([Reply::data(&[7u8; BS])]);
    let mut buf = vec![0u8; 2 * BS];
    assert_eq!(
        disk.read(0, &mut buf),
        Err(ScsiError::ShortTransfer {
            expected: 2 * BS,
            transferred: BS
        })
    );
}

#[test]
fn short_write_is_an_error() {
    let mut disk = disk([Reply::good().residual(1)]);
    assert_eq!(
        disk.write(0, &[0u8; BS]),
        Err(ScsiError::ShortTransfer {
            expected: BS,
            transferred: BS - 1
        })
    );
}

#[test]
fn residual_beyond_buffer_rejected() {
    let mut disk = disk([
        Reply::good().residual(BS as u32 + 1),
        Reply::good().residual(1),
    ]);
    assert_eq!(
        disk.write(0, &[0u8; BS]),
        Err(ScsiError::BadResidual {
            len: BS,
            residual: BS as u32 + 1
        })
    );
    // A residual on a command without a data phase is equally bogus.
    assert_eq!(
        disk.flush(),
        Err(ScsiError::BadResidual {
            len: 0,
            residual: 1
        })
    );
}

#[test]
fn check_condition_on_read_surfaces_sense() {
    let mut disk = disk([
        Reply::check(sense_key::MEDIUM_ERROR, 0x11, 0x00),
        Reply::status(scsi_block::status::CHECK_CONDITION),
    ]);
    let mut buf = vec![0u8; BS];
    let sense = Sense {
        key: sense_key::MEDIUM_ERROR,
        asc: 0x11,
        ascq: 0,
    };
    assert_eq!(
        disk.read(5, &mut buf),
        Err(ScsiError::CheckCondition(sense))
    );
    assert_eq!(
        disk.read(5, &mut buf),
        Err(ScsiError::CheckConditionNoSense)
    );
}
