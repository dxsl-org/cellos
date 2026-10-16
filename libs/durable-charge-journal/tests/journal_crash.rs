use cellos_fs::disk::MemDisk;
use durable_charge_journal::{Journal, JournalError, Superblock};

#[test]
fn journal_format_and_open() {
    let disk = MemDisk::new(128);
    let ledger_id = [1u8; 32];
    let genesis_id = [2u8; 32];
    let journal = Journal::format(disk.clone(), ledger_id, genesis_id, 1).expect("format");

    assert_eq!(journal.superblock().sequence, 1);
    assert_eq!(journal.superblock().head_revision, 0);
    assert_eq!(journal.superblock().ledger_id, ledger_id);

    // Reopen
    let reopened = Journal::open(disk).expect("reopen");
    assert_eq!(reopened.superblock().sequence, 1);
    assert_eq!(reopened.superblock().ledger_id, ledger_id);
}

#[test]
fn journal_append_records_and_recover() {
    let disk = MemDisk::new(128);
    let mut journal = Journal::format(disk.clone(), [1u8; 32], [2u8; 32], 1).expect("format");

    let record1 = [7u8; 300];
    let digest1 = [10u8; 32];
    let rev1 = journal.append_record(&record1, digest1).expect("append 1");
    assert_eq!(rev1, 1);
    assert_eq!(journal.superblock().sequence, 2);
    assert_eq!(journal.superblock().head_digest, digest1);

    let record2 = [8u8; 400];
    let digest2 = [11u8; 32];
    let rev2 = journal.append_record(&record2, digest2).expect("append 2");
    assert_eq!(rev2, 2);
    assert_eq!(journal.superblock().sequence, 3);
    assert_eq!(journal.superblock().head_digest, digest2);

    // Reopen from disk, verify state is recovered
    let recovered = Journal::open(disk).expect("open");
    assert_eq!(recovered.superblock().head_revision, 2);
    assert_eq!(recovered.superblock().sequence, 3);
    assert_eq!(recovered.superblock().head_digest, digest2);
}

#[test]
fn journal_crash_before_superblock_commit_retains_prior_head() {
    let disk = MemDisk::new(128);
    let mut journal = Journal::format(disk.clone(), [1u8; 32], [2u8; 32], 1).expect("format");

    let record1 = [7u8; 300];
    let digest1 = [10u8; 32];
    journal.append_record(&record1, digest1).expect("append 1");

    // Cut power right after data write, before superblock update (simulated by failing writes)
    let writes_before = disk.write_count();
    let mut crashing_disk = disk.clone();
    crashing_disk.set_power_cut(writes_before + 1); // Allow data block write, fail superblock write

    let mut crash_journal = Journal::open(crashing_disk).expect("open before crash");
    let record2 = [9u8; 300];
    let _ = crash_journal.append_record(&record2, [99u8; 32]);

    // Reopen after crash: prior head must remain intact!
    let recovered = Journal::open(disk).expect("recover");
    assert_eq!(recovered.superblock().head_revision, 1);
    assert_eq!(recovered.superblock().head_digest, digest1);
}
