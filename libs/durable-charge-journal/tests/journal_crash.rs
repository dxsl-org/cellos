mod support;

use cellos_fs::disk::MemDisk;
use durable_charge_journal::Journal;
use support::*;

#[test]
fn journal_format_and_open() {
    let disk = MemDisk::new(128);
    let journal = Journal::format(disk.clone(), LEDGER, GENESIS, EPOCH, &writer()).expect("format");
    assert_eq!(journal.superblock().sequence, 1);
    assert_eq!(journal.superblock().head_revision, 0);
    assert_eq!(
        journal.superblock().head_digest,
        GENESIS,
        "genesis id is the chain root"
    );

    let reopened = Journal::open(disk, &expect(1)).expect("reopen");
    assert_eq!(reopened.superblock(), journal.superblock());
}

#[test]
fn journal_append_records_and_recover() {
    let disk = MemDisk::new(128);
    let keys = writer();
    let mut journal = Journal::format(disk.clone(), LEDGER, GENESIS, EPOCH, &keys).expect("format");

    let record1 = record(1, GENESIS);
    assert_eq!(journal.append_record(&keys, &record1, &limits()), Ok(1));
    assert_eq!(journal.superblock().sequence, 2);
    assert_eq!(journal.superblock().head_digest, digest(&record1));

    let record2 = record(2, digest(&record1));
    assert_eq!(journal.append_record(&keys, &record2, &limits()), Ok(2));
    assert_eq!(journal.superblock().sequence, 3);

    let recovered = Journal::open(disk, &expect(3)).expect("open");
    assert_eq!(recovered.superblock().head_revision, 2);
    assert_eq!(recovered.superblock().head_digest, digest(&record2));
}

#[test]
fn multi_block_record_with_split_trailer_reopens() {
    let disk = MemDisk::new(16);
    let keys = writer();
    let mut journal = Journal::format(disk.clone(), LEDGER, GENESIS, EPOCH, &keys).expect("format");
    // 4100 bytes: the 32-byte digest trailer straddles the first block boundary.
    let big = encode_with(&header(1, GENESIS), 159);
    assert_eq!(big.len(), 4100);
    journal
        .append_record(&keys, &big, &limits())
        .expect("append");
    let small = record(2, digest(&big));
    journal
        .append_record(&keys, &small, &limits())
        .expect("append after two-block record");
    let recovered = Journal::open(disk, &expect(3)).expect("open");
    assert_eq!(recovered.superblock().head_record_block, 4);
}

#[test]
fn journal_crash_before_superblock_commit_retains_prior_head() {
    let disk = MemDisk::new(128);
    let keys = writer();
    let mut journal = Journal::format(disk.clone(), LEDGER, GENESIS, EPOCH, &keys).expect("format");
    let record1 = record(1, GENESIS);
    journal
        .append_record(&keys, &record1, &limits())
        .expect("append 1");

    // Power cut after the data block lands but before the superblock slot write.
    let mut crashing_disk = disk.clone();
    crashing_disk.set_power_cut(disk.write_count() + 1);
    let mut crash_journal = Journal::open(crashing_disk, &expect(2)).expect("open before crash");
    let record2 = record(2, digest(&record1));
    assert!(crash_journal
        .append_record(&keys, &record2, &limits())
        .is_err());

    let recovered = Journal::open(disk, &expect(2)).expect("recover");
    assert_eq!(recovered.superblock().head_revision, 1);
    assert_eq!(recovered.superblock().head_digest, digest(&record1));
}
