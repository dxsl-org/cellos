//! Fault points on import and delete, torn writes, the documented
//! non-guarantees, and the poison mode (K4b CP1 sections 4.7 and 4.9).

use std::vec::Vec;

use super::mem_io::{Call, ReadbackFault, WriteFault};
use super::*;
use crate::ssh::import::ImportSlot;
use crate::ssh::persist::record::{digest, parse};
use crate::ssh::LeafState;

fn expect_import_rolled_back(leaf: &FileLeaf) {
    assert!(leaf.identity.is_none());
    assert_eq!(leaf.revision, 0);
    assert!(leaf.arena.is_zeroed());
    assert!(matches!(leaf.import, ImportSlot::Idle));
    assert!(leaf.persist.head.is_none());
}

fn readiness_after_reboot(leaf: &FileLeaf) -> (KmsCapabilityReadiness, u16) {
    let mut next = reboot(leaf);
    let query = leaf_readiness(&mut next, &Actor::admin());
    (query.readiness, query.key_count)
}

#[test]
fn a_failed_write_on_import_changes_nothing_and_a_retry_works() {
    let admin = Actor::admin();
    for fault in [
        WriteFault::Fail,
        WriteFault::Tear(0),
        WriteFault::Tear(1),
        WriteFault::Tear(120),
        WriteFault::Tear(300),
    ] {
        let mut leaf = fresh();
        // Load first so only the write is faulted.
        assert_eq!(leaf_readiness(&mut leaf, &admin).key_count, 0);
        leaf.persist.io.write_fault = Some(fault);
        assert_eq!(
            import_cheap(&mut leaf, &admin),
            Err(KmsErrorCode::PersistFailed)
        );
        expect_import_rolled_back(&leaf);
        assert_eq!(leaf.state, LeafState::Serving);

        match fault {
            WriteFault::Fail => {
                assert!(leaf.persist.io.file(Slot::A).is_none());
                // Nothing on disk: a restart is an empty store.
                assert_eq!(
                    readiness_after_reboot(&leaf),
                    (KmsCapabilityReadiness::Ready, 0)
                );
            }
            WriteFault::Tear(n) => {
                assert_eq!(leaf.persist.io.file(Slot::A).map(Vec::len), Some(n));
                // A torn FIRST record leaves no valid record: visible, not
                // empty (the repair import is the way out).
                assert_eq!(
                    readiness_after_reboot(&leaf),
                    (KmsCapabilityReadiness::ProviderError, 0)
                );
            }
        }

        // Retry on the same leaf: the target slot is unchanged (A again).
        let ok = import_cheap(&mut leaf, &admin).unwrap();
        assert_eq!(ok.store_revision, 1);
        assert_eq!(ok.posture, KmsCustodyPosture::DevPosturePersistent);
        let record = bytes_of(&leaf, Slot::A);
        assert!(parse(Slot::A, &record).is_some());
        assert!(leaf.persist.io.file(Slot::B).is_none());
    }
}

#[test]
fn a_failed_ensure_dirs_on_store_is_persist_failed_with_no_write() {
    let admin = Actor::admin();
    // A leaf that never loaded (serving from the start) still ensures dirs
    // before its first write.
    let mut io = MemBlobIo::new();
    io.fail_ensure_dirs = true;
    let mut leaf = SshLeaf::serving(FilePersister::new(io));
    assert_eq!(
        import_blob(&mut leaf, &admin, NAME, &cheap_blob()),
        Err(KmsErrorCode::PersistFailed)
    );
    assert_eq!(leaf.persist.io.calls, std::vec![Call::EnsureDirs]);
    assert!(leaf.identity.is_none());
    // Once the directories can be made, the import goes through.
    leaf.persist.io.fail_ensure_dirs = false;
    import_blob(&mut leaf, &admin, NAME, &cheap_blob()).unwrap();
    assert!(leaf.persist.dirs_ready);
    assert!(leaf.persist.io.dirs);
}

#[test]
fn write_ok_but_readback_fails_leaves_a_record_a_new_leaf_loads() {
    for fault in [ReadbackFault::Error, ReadbackFault::Flip] {
        let admin = Actor::admin();
        let mut leaf = fresh();
        let _ = leaf_readiness(&mut leaf, &admin);
        leaf.persist.io.readback_fault = Some(fault);
        assert_eq!(
            import_cheap(&mut leaf, &admin),
            Err(KmsErrorCode::PersistFailed)
        );
        expect_import_rolled_back(&leaf);
        // Documented: the unacknowledged record is valid on disk and appears
        // after a restart.
        assert!(parse(Slot::A, &bytes_of(&leaf, Slot::A)).is_some());
        let mut next = reboot(&leaf);
        let named = status(&mut next, &Actor::admin(), NAME).unwrap();
        assert_eq!(named.public_key, cheap_public());
        assert_eq!(named.store_revision, 1);
        // A retry on the original leaf rewrites the same slot and succeeds.
        let ok = import_cheap(&mut leaf, &admin).unwrap();
        assert_eq!(ok.store_revision, 1);
    }
}

fn leaf_with_key(admin: &Actor) -> FileLeaf {
    let mut leaf = fresh();
    import_cheap(&mut leaf, admin).unwrap();
    assert_eq!(leaf.persist.head.unwrap().slot, Slot::A);
    leaf
}

#[test]
fn a_failed_tombstone_keeps_the_key_and_a_torn_one_resurrects_it() {
    let admin = Actor::admin();
    let login = Actor::login();
    for fault in [
        WriteFault::Fail,
        WriteFault::Tear(10),
        WriteFault::Tear(248),
    ] {
        let mut leaf = leaf_with_key(&admin);
        stage_secret(&mut leaf, &login, cheap_public(), PASS).unwrap();
        leaf.persist.io.write_fault = Some(fault);
        let before = snapshot(&mut leaf, &[&admin, &login]);
        assert_eq!(
            delete_key(&mut leaf, &admin, NAME, cheap_public()),
            Err(KmsErrorCode::PersistFailed)
        );
        assert_eq!(snapshot(&mut leaf, &[&admin, &login]), before);
        assert_eq!(leaf.persist.head.unwrap().slot, Slot::A);
        assert!(
            sign(&mut leaf, &login, cheap_public(), SESSION_ID, b"u").is_ok(),
            "the staged secret and the key survived"
        );
        // Documented non-guarantee, asserted as the expected outcome: the
        // delete was never acknowledged and the old record loads again.
        assert_eq!(
            readiness_after_reboot(&leaf),
            (KmsCapabilityReadiness::Ready, 1)
        );
        // Retry: the tombstone goes to the same inactive slot and succeeds.
        delete_key(&mut leaf, &admin, NAME, cheap_public()).unwrap();
        assert_eq!(
            readiness_after_reboot(&leaf),
            (KmsCapabilityReadiness::Ready, 0)
        );
    }
}

#[test]
fn a_tombstone_whose_readback_failed_is_on_disk() {
    let admin = Actor::admin();
    for fault in [ReadbackFault::Error, ReadbackFault::Flip] {
        let mut leaf = leaf_with_key(&admin);
        leaf.persist.io.readback_fault = Some(fault);
        assert_eq!(
            delete_key(&mut leaf, &admin, NAME, cheap_public()),
            Err(KmsErrorCode::PersistFailed)
        );
        assert!(leaf.identity.is_some(), "the key is kept in RAM");
        // Unacknowledged but complete: after a restart the key is gone.
        assert_eq!(
            readiness_after_reboot(&leaf),
            (KmsCapabilityReadiness::Ready, 0)
        );
    }
}

#[test]
fn a_torn_import_over_an_older_slot_keeps_the_older_state() {
    let admin = Actor::admin();
    let mut leaf = leaf_with_key(&admin);
    delete_key(&mut leaf, &admin, NAME, cheap_public()).unwrap();
    // A holds the key (rev 1), B the tombstone (rev 2). The next import goes
    // to A and is torn: the tombstone in B still loads.
    leaf.persist.io.write_fault = Some(WriteFault::Tear(200));
    assert_eq!(
        import_cheap(&mut leaf, &admin),
        Err(KmsErrorCode::PersistFailed)
    );
    assert_eq!(
        readiness_after_reboot(&leaf),
        (KmsCapabilityReadiness::Ready, 0)
    );
    // The in-flight import is simply lost; the retry succeeds at revision 3.
    let ok = import_cheap(&mut leaf, &admin).unwrap();
    assert_eq!(ok.store_revision, 3);
}

#[test]
fn records_alternate_and_chain() {
    let admin = Actor::admin();
    let mut leaf = fresh();
    import_cheap(&mut leaf, &admin).unwrap(); // A rev 1
    let a1 = bytes_of(&leaf, Slot::A);
    delete_key(&mut leaf, &admin, NAME, cheap_public()).unwrap(); // B rev 2
    let b2 = bytes_of(&leaf, Slot::B);
    import_cheap(&mut leaf, &admin).unwrap(); // A rev 3
    let a3 = bytes_of(&leaf, Slot::A);
    delete_key(&mut leaf, &admin, NAME, cheap_public()).unwrap(); // B rev 4
    let b4 = bytes_of(&leaf, Slot::B);

    fn p(slot: Slot, bytes: &[u8]) -> crate::ssh::persist::record::Parsed<'_> {
        parse(slot, bytes).unwrap()
    }
    assert_eq!(p(Slot::A, &a1).previous, [0; 32]);
    assert_eq!(p(Slot::B, &b2).previous, digest(&a1));
    assert_eq!(p(Slot::A, &a3).previous, digest(&b2));
    assert_eq!(p(Slot::B, &b4).previous, digest(&a3));
    assert_eq!(
        [
            p(Slot::A, &a1).revision,
            p(Slot::B, &b2).revision,
            p(Slot::A, &a3).revision,
            p(Slot::B, &b4).revision
        ],
        [1, 2, 3, 4]
    );
    assert!(p(Slot::A, &a1).present && !p(Slot::B, &b2).present && p(Slot::A, &a3).present);
    // Only the two files were ever written, one write per operation, each
    // preceded by directory setup once.
    assert_eq!(leaf.persist.io.writes(), 4);
    assert_eq!(
        leaf.persist
            .io
            .calls
            .iter()
            .filter(|c| **c == Call::EnsureDirs)
            .count(),
        1
    );
}

#[test]
fn poison_mode_fails_every_later_call_and_the_leaf_stays_unloaded_after_restart() {
    let admin = Actor::admin();
    let mut leaf = leaf_with_key(&admin);
    leaf.persist.io.poison_on_failure = true;
    leaf.persist.io.write_fault = Some(WriteFault::Fail);
    assert_eq!(
        delete_key(&mut leaf, &admin, NAME, cheap_public()),
        Err(KmsErrorCode::PersistFailed)
    );
    assert!(leaf.persist.io.poisoned);
    // Every later persist call fails too (no retry logic; poison persists
    // until the whole system reboots).
    for _ in 0..3 {
        assert_eq!(
            delete_key(&mut leaf, &admin, NAME, cheap_public()),
            Err(KmsErrorCode::PersistFailed)
        );
    }
    assert!(leaf.identity.is_some());
    assert_eq!(leaf.revision, 1);

    // A KMS restart over the still-poisoned backend cannot load.
    let mut restarted = leaf_over(leaf.persist.io.clone());
    assert_eq!(
        status(&mut restarted, &admin, NAME),
        Err(KmsErrorCode::SshKeyUnavailable)
    );
    assert_eq!(restarted.state, LeafState::NotLoaded);
    assert_eq!(
        leaf_readiness(&mut restarted, &admin).readiness,
        KmsCapabilityReadiness::Unavailable
    );
    // Imports are not admitted past an unloaded leaf either.
    assert_eq!(
        begin_import(&mut restarted, &admin, NAME, 10, [0; 32]),
        Err(KmsErrorCode::SshKeyUnavailable)
    );

    // After a real reboot the same files load (the delete never landed).
    let mut rebooted = reboot(&leaf);
    assert_eq!(
        status(&mut rebooted, &admin, NAME).unwrap().store_revision,
        1
    );
}

#[test]
fn poison_on_the_first_import_is_persist_failed_then_unavailable() {
    let admin = Actor::admin();
    let mut io = MemBlobIo::new();
    io.poison_on_failure = true;
    io.write_fault = Some(WriteFault::Tear(50));
    let mut leaf = leaf_over(io);
    assert_eq!(
        import_cheap(&mut leaf, &admin),
        Err(KmsErrorCode::PersistFailed)
    );
    assert!(leaf.persist.io.poisoned);
    assert_eq!(
        import_cheap(&mut leaf, &admin),
        Err(KmsErrorCode::PersistFailed)
    );
    let mut restarted = leaf_over(leaf.persist.io.clone());
    assert_eq!(
        status(&mut restarted, &admin, NAME),
        Err(KmsErrorCode::SshKeyUnavailable)
    );
}
