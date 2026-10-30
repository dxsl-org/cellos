//! Load classification (K4b CP1 section 4.9): every row of the table, through
//! `SshLeaf::new(FilePersister<MemBlobIo>)`.

use std::vec::Vec;

use super::mem_io::Call;
use super::record::Raw;
use super::*;
use crate::ssh::persist::record::digest;
use crate::ssh::LeafState;

fn io_with(a: Option<&[u8]>, b: Option<&[u8]>) -> MemBlobIo {
    let mut io = MemBlobIo::new();
    io.dirs = true;
    if let Some(bytes) = a {
        io.put(Slot::A, bytes);
    }
    if let Some(bytes) = b {
        io.put(Slot::B, bytes);
    }
    io
}

/// Present record in A (revision 1), no predecessor.
fn first_present() -> Vec<u8> {
    Raw::present(Slot::A).bytes()
}

/// Tombstone in B (revision 2) chained onto `first_present()`.
fn tombstone_after_first() -> Vec<u8> {
    let mut raw = Raw::tombstone(Slot::B);
    raw.revision = 2;
    raw.previous = digest(&first_present());
    raw.bytes()
}

/// A present record in B (revision 2) chained onto `first_present()`.
fn present_b_after_first() -> Vec<u8> {
    let mut raw = Raw::present(Slot::B);
    raw.revision = 2;
    raw.previous = digest(&first_present());
    raw.bytes()
}

fn expect_found(leaf: &mut FileLeaf, revision: u64) {
    let admin = Actor::admin();
    let named = status(leaf, &admin, NAME).unwrap();
    assert_eq!(named.key_present, 1);
    assert_eq!(named.public_key, cheap_public());
    assert_eq!(named.custody, KmsSshKeyCustody::PassphraseWrapped);
    assert_eq!(named.posture, KmsCustodyPosture::DevPosturePersistent);
    assert_eq!(named.provider, KmsProviderKind::None);
    assert_eq!(named.readiness, KmsCapabilityReadiness::Ready);
    assert_eq!(named.store_revision, revision);
    assert_eq!(leaf.state, LeafState::Serving);
}

fn expect_empty_serving(leaf: &mut FileLeaf) {
    let admin = Actor::admin();
    let query = leaf_readiness(leaf, &admin);
    assert_eq!(query.readiness, KmsCapabilityReadiness::Ready);
    assert_eq!(query.key_count, 0);
    assert_eq!(leaf.state, LeafState::Serving);
    assert_eq!(
        status(leaf, &admin, NAME),
        Err(KmsErrorCode::SshKeyNotFound)
    );
}

#[test]
fn neither_file_exists_is_empty_and_serving() {
    let mut leaf = fresh();
    assert_eq!(leaf.state, LeafState::NotLoaded);
    expect_empty_serving(&mut leaf);
    // With an empty store the leaf query reads the volatile posture (the
    // posture describes the stored key).
    let admin = Actor::admin();
    let query = leaf_readiness(&mut leaf, &admin);
    assert_eq!(query.posture, KmsCustodyPosture::DevPostureVolatile);
    assert_eq!(query.store_revision, 0);
    assert!(leaf.persist.head.is_none());
    // One pass: dirs, then both slots; one load only.
    assert_eq!(
        leaf.persist.io.calls,
        std::vec![Call::EnsureDirs, Call::Read(Slot::A), Call::Read(Slot::B)]
    );
    // Load does not write.
    assert_eq!(leaf.persist.io.writes(), 0);
}

#[test]
fn exactly_one_valid_record_loads_whichever_slot_holds_it() {
    let a = first_present();
    let mut only_a = leaf_over(io_with(Some(&a), None));
    expect_found(&mut only_a, 1);
    let head = only_a.persist.head.unwrap();
    assert_eq!((head.slot, head.present), (Slot::A, true));

    let mut b = Raw::present(Slot::B);
    b.revision = 5;
    let mut only_b = leaf_over(io_with(None, Some(&b.bytes())));
    expect_found(&mut only_b, 5);
    assert_eq!(only_b.persist.head.unwrap().slot, Slot::B);
}

#[test]
fn a_torn_other_slot_does_not_hide_the_valid_one() {
    let a = first_present();
    // The newer write to B was torn at every interesting length.
    let newer = present_b_after_first();
    for keep in [0usize, 1, 4, 100, 248, newer.len() - 1] {
        let mut leaf = leaf_over(io_with(Some(&a), Some(&newer[..keep])));
        expect_found(&mut leaf, 1);
        // The next record goes to the torn slot.
        assert_eq!(leaf.persist.head.unwrap().slot, Slot::A);
    }
    // And the mirror: B valid, A torn.
    let mut b = Raw::present(Slot::B);
    b.revision = 2;
    let b = b.bytes();
    let mut leaf = leaf_over(io_with(Some(&a[..77]), Some(&b)));
    expect_found(&mut leaf, 2);
}

#[test]
fn one_valid_tombstone_loads_empty() {
    let tomb = Raw::tombstone(Slot::A).bytes();
    let mut leaf = leaf_over(io_with(Some(&tomb), None));
    expect_empty_serving(&mut leaf);
    let head = leaf.persist.head.unwrap();
    assert_eq!((head.slot, head.present), (Slot::A, false));
}

#[test]
fn chained_pair_the_successor_wins_in_both_orders() {
    // B succeeds A: a tombstone => empty.
    let mut leaf = leaf_over(io_with(
        Some(&first_present()),
        Some(&tombstone_after_first()),
    ));
    expect_empty_serving(&mut leaf);
    assert_eq!(leaf.persist.head.unwrap().slot, Slot::B);

    // B succeeds A with a key => found, revision 2.
    let mut leaf = leaf_over(io_with(
        Some(&first_present()),
        Some(&present_b_after_first()),
    ));
    expect_found(&mut leaf, 2);
    assert_eq!(leaf.persist.head.unwrap().slot, Slot::B);

    // A succeeds B: A (rev 3) chained onto the tombstone in B.
    let tomb = tombstone_after_first();
    let mut a3 = Raw::present(Slot::A);
    a3.revision = 3;
    a3.previous = digest(&tomb);
    let mut leaf = leaf_over(io_with(Some(&a3.bytes()), Some(&tomb)));
    expect_found(&mut leaf, 3);
    assert_eq!(leaf.persist.head.unwrap().slot, Slot::A);
}

#[test]
fn revision_numbers_do_not_order_the_chain() {
    // The successor has the LOWER revision (a repair import restarts at 1).
    let mut old = Raw::present(Slot::A);
    old.revision = 40;
    let old = old.bytes();
    let mut newer = Raw::present(Slot::B);
    newer.revision = 1;
    newer.previous = digest(&old);
    let mut leaf = leaf_over(io_with(Some(&old), Some(&newer.bytes())));
    expect_found(&mut leaf, 1);
    assert_eq!(leaf.persist.head.unwrap().slot, Slot::B);
}

#[test]
fn two_valid_unchained_records_are_a_rollback_tripwire() {
    let admin = Actor::admin();
    let login = Actor::login();
    // Zero predecessors on both: neither chains onto the other.
    let a = Raw::present(Slot::A).bytes();
    let mut b = Raw::present(Slot::B);
    b.revision = 7;
    let mut leaf = leaf_over(io_with(Some(&a), Some(&b.bytes())));
    let query = leaf_readiness(&mut leaf, &admin);
    assert_eq!(query.readiness, KmsCapabilityReadiness::PolicyMismatch);
    assert_eq!(query.key_count, 0);
    assert_eq!(
        leaf.state,
        LeafState::Repairable(KmsCapabilityReadiness::PolicyMismatch)
    );
    // The higher revision is the predecessor of a repair record; the other
    // slot is overwritten.
    assert_eq!(leaf.persist.head.unwrap().slot, Slot::B);
    assert_eq!(leaf.persist.repair_target, Some(Slot::A));
    // Only import (15-18) is admitted; everything else is unavailable.
    assert_eq!(
        status(&mut leaf, &admin, NAME),
        Err(KmsErrorCode::SshKeyUnavailable)
    );
    assert_eq!(
        delete_key(&mut leaf, &admin, NAME, cheap_public()),
        Err(KmsErrorCode::SshKeyUnavailable)
    );
    assert_eq!(
        unlock_begin(&mut leaf, &login, cheap_public(), 4),
        Err(KmsErrorCode::SshKeyUnavailable)
    );
    assert_eq!(
        sign(&mut leaf, &login, cheap_public(), SESSION_ID, b"user"),
        Err(KmsErrorCode::SshKeyUnavailable)
    );
    let blob = cheap_blob();
    assert!(begin_import(&mut leaf, &admin, NAME, blob.len() as u32, sha(&blob)).is_ok());
    // The tie rule: equal revisions keep A, overwrite B.
    let mut tied = Raw::present(Slot::B);
    tied.revision = 1;
    let mut leaf = leaf_over(io_with(Some(&a), Some(&tied.bytes())));
    let _ = leaf_readiness(&mut leaf, &admin);
    assert_eq!(leaf.persist.head.unwrap().slot, Slot::A);
    assert_eq!(leaf.persist.repair_target, Some(Slot::B));
}

#[test]
fn no_valid_record_is_corrupt_not_empty() {
    let admin = Actor::admin();
    let good = first_present();
    let mut flipped = good.clone();
    flipped[200] ^= 1;
    let oversize = std::vec![0u8; RECORD_MAX + 1];
    type Case<'a> = (&'a str, Option<&'a [u8]>, Option<&'a [u8]>);
    let cases: [Case<'_>; 6] = [
        ("garbage A only", Some(b"garbage"), None),
        ("garbage B only", None, Some(b"garbage")),
        ("empty file", Some(b""), None),
        ("bit flip in A", Some(&flipped), None),
        ("both invalid", Some(&flipped), Some(&good[..100])),
        ("oversize", None, Some(&oversize)),
    ];
    for (what, a, b) in cases {
        let mut leaf = leaf_over(io_with(a, b));
        let query = leaf_readiness(&mut leaf, &admin);
        assert_eq!(
            query.readiness,
            KmsCapabilityReadiness::ProviderError,
            "{what}"
        );
        assert_eq!(
            leaf.state,
            LeafState::Repairable(KmsCapabilityReadiness::ProviderError),
            "{what}"
        );
        assert_eq!(leaf.persist.repair_target, Some(Slot::A), "{what}");
        assert!(leaf.persist.head.is_none(), "{what}");
        assert_eq!(
            status(&mut leaf, &admin, NAME),
            Err(KmsErrorCode::SshKeyUnavailable),
            "{what}"
        );
    }
}

#[test]
fn a_v1_relay_slot_file_under_an_ssh_name_is_corrupt() {
    use crate::storage::{JournalRecord, SlotId};
    let v1 = JournalRecord::placeholder(SlotId::A, 1, 1, [0; 32])
        .encode(&crate::ssh::persist::devroot::mac_key());
    let admin = Actor::admin();
    let mut leaf = leaf_over(io_with(Some(&v1), None));
    assert_eq!(
        leaf_readiness(&mut leaf, &admin).readiness,
        KmsCapabilityReadiness::ProviderError
    );
}

#[test]
fn a_record_for_the_other_slot_is_corrupt() {
    // ssh-a.bin holds a record that says "slot B" (a file copied over).
    let admin = Actor::admin();
    let wrong = Raw::present(Slot::B).bytes();
    let mut leaf = leaf_over(io_with(Some(&wrong), None));
    assert_eq!(
        leaf_readiness(&mut leaf, &admin).readiness,
        KmsCapabilityReadiness::ProviderError
    );
}

#[test]
fn blob_errors_are_io_errors_and_the_next_opcode_retries() {
    let admin = Actor::admin();
    let a = first_present();

    // ensure_dirs fails.
    let mut io = io_with(Some(&a), None);
    io.fail_ensure_dirs = true;
    let mut leaf = leaf_over(io);
    assert_eq!(
        status(&mut leaf, &admin, NAME),
        Err(KmsErrorCode::SshKeyUnavailable)
    );
    assert_eq!(leaf.state, LeafState::NotLoaded);
    assert_eq!(
        leaf_readiness(&mut leaf, &admin).readiness,
        KmsCapabilityReadiness::Unavailable
    );
    leaf.persist.io.fail_ensure_dirs = false;
    expect_found(&mut leaf, 1);

    // read(A) fails, then read(B) fails: each leaves the leaf unloaded, and a
    // later pass loads.
    for failing in [Slot::A, Slot::B] {
        let mut leaf = leaf_over(io_with(Some(&a), None));
        leaf.persist.io.fail_read[usize::from(failing.id())] = true;
        assert_eq!(
            status(&mut leaf, &admin, NAME),
            Err(KmsErrorCode::SshKeyUnavailable)
        );
        assert_eq!(leaf.state, LeafState::NotLoaded);
        assert!(leaf.persist.head.is_none());
        leaf.persist.io.fail_read = [false; 2];
        expect_found(&mut leaf, 1);
    }
}

#[test]
fn a_failed_load_retries_the_whole_pass_on_each_opcode() {
    let admin = Actor::admin();
    let mut io = MemBlobIo::new();
    io.fail_ensure_dirs = true;
    let mut leaf = leaf_over(io);
    for _ in 0..3 {
        assert_eq!(
            status(&mut leaf, &admin, NAME),
            Err(KmsErrorCode::SshKeyUnavailable)
        );
    }
    let attempts = leaf
        .persist
        .io
        .calls
        .iter()
        .filter(|call| **call == Call::EnsureDirs)
        .count();
    assert_eq!(attempts, 3);
    assert_eq!(leaf.persist.io.writes(), 0);
    // Directories were never made, so a write still has to ensure them.
    assert!(!leaf.persist.dirs_ready);
}

#[test]
fn every_truncation_of_a_record_is_invalid() {
    let admin = Actor::admin();
    for full in [first_present(), tombstone_after_first_in_a()] {
        for keep in 0..full.len() {
            let mut leaf = leaf_over(io_with(Some(&full[..keep]), None));
            assert_eq!(
                leaf_readiness(&mut leaf, &admin).readiness,
                KmsCapabilityReadiness::ProviderError,
                "truncated to {keep} of {}",
                full.len()
            );
        }
        // Control: the whole record loads.
        let mut leaf = leaf_over(io_with(Some(&full), None));
        assert_eq!(
            leaf_readiness(&mut leaf, &admin).readiness,
            KmsCapabilityReadiness::Ready
        );
    }
}

fn tombstone_after_first_in_a() -> Vec<u8> {
    Raw::tombstone(Slot::A).bytes()
}

#[test]
fn every_bit_flip_in_a_stored_record_is_corrupt() {
    let admin = Actor::admin();
    let good = first_present();
    for at in (0..good.len()).step_by(3) {
        let mut bytes = good.clone();
        bytes[at] ^= 0x10;
        let mut leaf = leaf_over(io_with(Some(&bytes), None));
        assert_eq!(
            leaf_readiness(&mut leaf, &admin).readiness,
            KmsCapabilityReadiness::ProviderError,
            "flip at {at}"
        );
    }
}
