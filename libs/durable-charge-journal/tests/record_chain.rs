//! H-35/S-6 regressions: head record re-verification, append chain checks, last block.
mod support;

use cellos_fs::disk::MemDisk;
use durable_charge_codec::{Error as CodecError, Header};
use durable_charge_journal::{Journal, JournalError};
use ed25519_compact::{KeyPair, Seed};
use support::*;

fn edited(revision: u64, previous: [u8; 32], edit: impl FnOnce(&mut Header)) -> Vec<u8> {
    let mut h = header(revision, previous);
    edit(&mut h);
    encode_with(&h, 1)
}

#[test]
fn tampered_or_substituted_head_record_is_refused() {
    let (disk, r1, _) = two_record_journal();
    let original = block(&disk, 3);
    let mut torn = original;
    torn[100] ^= 1;
    put(&disk, 3, &torn);
    let open = |disk: &MemDisk| Journal::open(disk.clone(), &expect(3)).err();
    assert_eq!(open(&disk), Some(JournalError::HeadRecord));

    // A structurally valid record with its own correct trailer is still not the head.
    let substitute = edited(2, digest(&r1), |h| h.local_transaction = [4; 16]);
    let mut other = [0u8; 4096];
    other[..substitute.len()].copy_from_slice(&substitute);
    put(&disk, 3, &other);
    assert_eq!(open(&disk), Some(JournalError::HeadRecord));

    put(&disk, 3, &original);
    assert_eq!(open(&disk), None, "restored head verifies");
}

#[test]
fn append_requires_decoded_record_linked_to_head() {
    let (disk, _, r2) = two_record_journal();
    let keys = writer();
    let mut journal = Journal::open(disk.clone(), &expect(3)).unwrap();
    let head = digest(&r2);
    let writes = disk.write_count();
    let mut bad_trailer = record(3, head);
    *bad_trailer.last_mut().unwrap() ^= 1;
    let cases = [
        (record(3, [8; 32]), JournalError::Chain),
        (record(4, head), JournalError::Chain),
        (record(2, head), JournalError::Chain),
        (
            edited(3, head, |h| h.journal_writer_epoch = EPOCH + 1),
            JournalError::StaleWriter,
        ),
        (
            edited(3, head, |h| h.journal_ledger = [5; 32]),
            JournalError::Identity,
        ),
        (bad_trailer, JournalError::Record(CodecError::Digest)),
    ];
    for (bytes, error) in cases {
        assert_eq!(journal.append_record(&keys, &bytes, &limits()), Err(error));
    }
    let intruder = KeyPair::from_seed(Seed::new([9; 32]));
    assert_eq!(
        journal.append_record(&intruder, &record(3, head), &limits()),
        Err(JournalError::WriterKey)
    );
    assert_eq!(
        disk.write_count(),
        writes,
        "refused appends must not touch the device"
    );
    assert_eq!(
        journal.append_record(&keys, &record(3, head), &limits()),
        Ok(3)
    );
}

#[test]
fn record_ending_on_the_last_block_reopens() {
    let disk = MemDisk::new(4); // data blocks 2 and 3 only
    let keys = writer();
    let mut journal = Journal::format(disk.clone(), LEDGER, GENESIS, EPOCH, &keys).unwrap();
    let r1 = record(1, GENESIS);
    journal.append_record(&keys, &r1, &limits()).unwrap();
    let r2 = record(2, digest(&r1));
    journal.append_record(&keys, &r2, &limits()).unwrap();
    assert_eq!(journal.superblock().next_block, 4);
    assert_eq!(
        journal.append_record(&keys, &record(3, digest(&r2)), &limits()),
        Err(JournalError::Capacity)
    );
    let reopened = Journal::open(disk, &expect(3)).expect("full journal reopens");
    assert_eq!(reopened.superblock().head_revision, 2);
}
