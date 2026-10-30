//! D-4b.2: a `Plain` identity is never written; the persister does not even
//! read the seed.

use super::mem_io::Call;
use super::*;
use crate::ssh::persist::FilePersister;
use crate::ssh::store::StoredIdentity;
use crate::ssh::{LeafState, SshPersist, Stored};

fn plain_blob() -> std::vec::Vec<u8> {
    id_ed25519()
}

#[test]
fn a_plain_import_makes_zero_writes_and_is_volatile() {
    let admin = Actor::admin();
    let mut leaf = fresh();
    let status = import_blob(&mut leaf, &admin, b"pk", &plain_blob()).unwrap();
    assert_eq!(status.custody, KmsSshKeyCustody::Plain);
    assert_eq!(status.posture, KmsCustodyPosture::DevPostureVolatile);
    assert_eq!(leaf.persist.io.writes(), 0);
    assert!(leaf.persist.io.received.is_empty());
    assert!(leaf.persist.io.file(Slot::A).is_none());
    assert!(leaf.persist.io.file(Slot::B).is_none());
    // The key serves from RAM and is gone after a restart.
    assert_eq!(
        status_of(&mut leaf).posture,
        KmsCustodyPosture::DevPostureVolatile
    );
    let mut restarted = reboot(&leaf);
    let query = leaf_readiness(&mut restarted, &admin);
    assert_eq!(query.readiness, KmsCapabilityReadiness::Ready);
    assert_eq!(query.key_count, 0);
}

fn status_of(leaf: &mut FileLeaf) -> types::kms::SshKeyStatusPayload {
    status(leaf, &Actor::admin(), b"pk").unwrap()
}

#[test]
fn a_plain_delete_makes_zero_writes() {
    let admin = Actor::admin();
    let mut leaf = fresh();
    import_blob(&mut leaf, &admin, b"pk", &plain_blob()).unwrap();
    delete_key(&mut leaf, &admin, b"pk", id_ed25519_public()).unwrap();
    assert_eq!(leaf.persist.io.writes(), 0);
    assert!(leaf.identity.is_none());
    assert_eq!(leaf.revision, 2);
}

#[test]
fn a_plain_import_after_a_persisted_delete_does_not_resurrect_anything() {
    let admin = Actor::admin();
    let mut leaf = fresh();
    import_cheap(&mut leaf, &admin).unwrap();
    delete_key(&mut leaf, &admin, NAME, cheap_public()).unwrap();
    let writes_before = leaf.persist.io.writes();
    assert_eq!(writes_before, 2);

    import_blob(&mut leaf, &admin, b"pk", &plain_blob()).unwrap();
    assert_eq!(leaf.persist.io.writes(), writes_before, "plain: no write");
    // Deleting the RAM-only key needs no tombstone: the persisted tombstone is
    // already the newest record.
    delete_key(&mut leaf, &admin, b"pk", id_ed25519_public()).unwrap();
    assert_eq!(leaf.persist.io.writes(), writes_before);

    let mut restarted = reboot(&leaf);
    let query = leaf_readiness(&mut restarted, &admin);
    assert_eq!(query.key_count, 0);
    assert_eq!(query.readiness, KmsCapabilityReadiness::Ready);
}

#[test]
fn a_plain_repair_import_persists_nothing_so_a_corrupt_store_stays_corrupt() {
    let admin = Actor::admin();
    let mut io = MemBlobIo::new();
    io.put(Slot::A, b"junk");
    let mut leaf = leaf_over(io);
    assert_eq!(
        leaf_readiness(&mut leaf, &admin).readiness,
        KmsCapabilityReadiness::ProviderError
    );
    import_blob(&mut leaf, &admin, b"pk", &plain_blob()).unwrap();
    assert_eq!(leaf.state, LeafState::Serving);
    assert_eq!(leaf.persist.io.writes(), 0);
    // Repairing a corrupt store durably needs a wrapped key.
    let mut restarted = reboot(&leaf);
    assert_eq!(
        leaf_readiness(&mut restarted, &admin).readiness,
        KmsCapabilityReadiness::ProviderError
    );
}

#[test]
fn the_persister_never_touches_storage_for_a_plain_record() {
    let identity =
        StoredIdentity::plain(b"pk", public_of(&RFC8032_SEED_1), &RFC8032_SEED_1).unwrap();
    let mut persister = FilePersister::new(MemBlobIo::new());
    assert_eq!(
        persister.store(&identity.record_view(1)),
        Ok(Stored::VolatileOnly)
    );
    // No ensure_dirs, no read, no write: nothing to observe the seed through.
    assert_eq!(persister.io.calls, std::vec::Vec::<Call>::new());
    assert!(!persister.dirs_ready);
    // A tombstone with nothing persisted is volatile too.
    assert_eq!(persister.tombstone(2), Ok(Stored::VolatileOnly));
    assert!(persister.io.calls.is_empty());
}
