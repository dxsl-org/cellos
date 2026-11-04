//! Probe steps after LUN selection: TEST UNIT READY and READ CAPACITY(16).
mod common;

use common::{capacity_data, lun0_disk, ready_tail, FakeError, FakeExecutor, Reply, CONFIG};
use scsi_block::cdb::OP_TEST_UNIT_READY;
use scsi_block::{sense_key, ParseError, ProbeConfig, ScsiDisk, ScsiError, Sense};

fn probe_with_tur(
    tur: impl IntoIterator<Item = Reply>,
    retries: u32,
) -> (FakeExecutor, Option<ScsiError<FakeError>>) {
    let mut fake = lun0_disk();
    fake.script.extend(tur);
    fake.script.push_back(Reply::data(&capacity_data(16, 512)));
    let config = ProbeConfig {
        tur_retries: retries,
        ..CONFIG
    };
    let err = ScsiDisk::probe(&mut fake, 0, config).err();
    (fake, err)
}

fn tur_count(fake: &FakeExecutor) -> usize {
    fake.opcodes()
        .iter()
        .filter(|&&op| op == OP_TEST_UNIT_READY)
        .count()
}

#[test]
fn tur_retries_unit_attention_and_becoming_ready() {
    let (fake, err) = probe_with_tur(
        [
            Reply::check(sense_key::UNIT_ATTENTION, 0x29, 0x00),
            Reply::check(sense_key::NOT_READY, 0x04, 0x01),
            Reply::good(),
        ],
        2,
    );
    assert_eq!(err, None);
    assert_eq!(tur_count(&fake), 3);
}

#[test]
fn tur_retries_exhausted() {
    let ua = Reply::check(sense_key::UNIT_ATTENTION, 0x29, 0x00);
    let (fake, err) = probe_with_tur([ua.clone(), ua.clone(), ua], 1);
    let sense = Sense {
        key: sense_key::UNIT_ATTENTION,
        asc: 0x29,
        ascq: 0,
    };
    assert_eq!(err, Some(ScsiError::NotReady(sense)));
    assert_eq!(tur_count(&fake), 2);
}

#[test]
fn tur_fatal_sense_is_not_retried() {
    let (fake, err) = probe_with_tur([Reply::check(sense_key::NOT_READY, 0x3A, 0x00)], 5);
    let sense = Sense {
        key: sense_key::NOT_READY,
        asc: 0x3A,
        ascq: 0,
    };
    assert_eq!(err, Some(ScsiError::CheckCondition(sense)));
    assert_eq!(tur_count(&fake), 1);
}

#[test]
fn bad_capacity_is_rejected() {
    let mut fake = lun0_disk();
    fake.script.extend(ready_tail(16, 520));
    assert_eq!(
        ScsiDisk::probe(fake, 0, CONFIG).err(),
        Some(ScsiError::Parse(ParseError::BadBlockSize(520)))
    );
}

#[test]
fn short_capacity_is_rejected() {
    let mut fake = lun0_disk();
    fake.script.extend([Reply::good(), Reply::data(&[0; 8])]);
    assert_eq!(
        ScsiDisk::probe(fake, 0, CONFIG).err(),
        Some(ScsiError::Parse(ParseError::CapacityTooShort(8)))
    );
}

#[test]
fn transfer_limit_below_block_size() {
    let mut fake = lun0_disk();
    fake.script.extend(ready_tail(16, 4096));
    let config = ProbeConfig {
        max_transfer_bytes: 4095,
        ..CONFIG
    };
    assert_eq!(
        ScsiDisk::probe(fake, 0, config).err(),
        Some(ScsiError::TransferLimitBelowBlockSize {
            max_transfer_bytes: 4095,
            block_size: 4096
        })
    );
}
