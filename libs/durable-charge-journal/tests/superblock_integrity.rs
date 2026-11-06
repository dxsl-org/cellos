//! H-35 regressions: superblock slots are writer-signed, ledger-bound and floor-checked.
mod support;

use cellos_fs::disk::MemDisk;
use durable_charge_journal::{Journal, JournalError, Superblock};
use ed25519_compact::{KeyPair, Seed};
use support::*;

fn open_err(disk: &MemDisk, min_sequence: u64) -> Option<JournalError> {
    Journal::open(disk.clone(), &expect(min_sequence)).err()
}

#[test]
fn forged_slot_is_ignored_and_floor_refuses_the_fallback() {
    let (disk, _, _) = two_record_journal();
    // v1 attack: take an older slot, bump its sequence; without the writer key the
    // signature no longer verifies, so the slot is treated as torn.
    let mut forged = block(&disk, 1);
    forged[8..16].copy_from_slice(&99u64.to_le_bytes());
    put(&disk, 0, &forged);
    assert_eq!(open_err(&disk, 3), Some(JournalError::Rollback));
    let fallback = Journal::open(disk.clone(), &expect(2)).expect("older signed slot");
    let sb = fallback.superblock();
    assert_eq!((sb.sequence, sb.head_revision), (2, 1));

    put(&disk, 1, &forged);
    assert_eq!(open_err(&disk, 0), Some(JournalError::Signature));
}

#[test]
fn slots_signed_by_another_key_are_refused() {
    let disk = MemDisk::new(16);
    let attacker = KeyPair::from_seed(Seed::new([9; 32]));
    Journal::format(disk.clone(), LEDGER, GENESIS, EPOCH, &attacker).unwrap();
    assert_eq!(open_err(&disk, 0), Some(JournalError::Signature));
}

#[test]
fn replayed_older_signed_slot_is_refused_by_floor() {
    let disk = MemDisk::new(16);
    let keys = writer();
    let mut journal = Journal::format(disk.clone(), LEDGER, GENESIS, EPOCH, &keys).unwrap();
    let genesis_slot = block(&disk, 0);
    let r1 = record(1, GENESIS);
    journal.append_record(&keys, &r1, &limits()).unwrap();
    journal
        .append_record(&keys, &record(2, digest(&r1)), &limits())
        .unwrap();
    put(&disk, 0, &genesis_slot);
    assert_eq!(open_err(&disk, 3), Some(JournalError::Rollback));
}

#[test]
fn spliced_foreign_ledger_slot_is_refused() {
    let disk = MemDisk::new(16);
    Journal::format(disk.clone(), LEDGER, GENESIS, EPOCH, &writer()).unwrap();
    let foreign = MemDisk::new(16);
    Journal::format(foreign.clone(), [5; 32], GENESIS, EPOCH, &writer()).unwrap();
    put(&disk, 1, &block(&foreign, 0));
    assert_eq!(open_err(&disk, 0), Some(JournalError::Identity));

    put(&disk, 1, &[0; 4096]);
    let wrong_genesis = durable_charge_journal::JournalExpectation {
        genesis_id: [6; 32],
        ..expect(0)
    };
    assert_eq!(
        Journal::open(disk, &wrong_genesis).err(),
        Some(JournalError::Identity)
    );
}

#[test]
fn equivocating_slots_with_equal_sequence_are_refused() {
    let disk = MemDisk::new(16);
    let keys = writer();
    let journal = Journal::format(disk.clone(), LEDGER, GENESIS, EPOCH, &keys).unwrap();
    let fork = Superblock {
        writer_epoch: EPOCH + 1,
        ..*journal.superblock()
    };
    let mut buf = [0u8; 4096];
    fork.encode(&keys.sk, &mut buf);
    put(&disk, 1, &buf);
    assert_eq!(open_err(&disk, 0), Some(JournalError::Corrupt));
}

#[test]
fn version_one_superblocks_are_refused() {
    let disk = MemDisk::new(16);
    let mut v1 = [0u8; 4096];
    v1[0..4].copy_from_slice(b"DCOJ");
    v1[4..6].copy_from_slice(&1u16.to_le_bytes());
    put(&disk, 0, &v1);
    put(&disk, 1, &v1);
    assert_eq!(open_err(&disk, 0), Some(JournalError::Version));
}
