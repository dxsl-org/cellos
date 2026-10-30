//! The persister behind the ten handlers: import, restore, unlock and sign,
//! delete, repair, retry, and what a restart does not carry over.

use super::mem_io::Call;
use super::record::Raw;
use super::*;
use crate::ssh::persist::record::{digest, parse};
use crate::ssh::unlock::UnlockSlot;
use crate::ssh::LeafState;

#[test]
fn import_writes_one_record_and_reads_it_back_before_replying() {
    let admin = Actor::admin();
    let mut leaf = fresh();
    let status = import_cheap(&mut leaf, &admin).unwrap();
    assert_eq!(status.posture, KmsCustodyPosture::DevPosturePersistent);
    assert_eq!(status.provider, KmsProviderKind::None);
    assert_eq!(status.custody, KmsSshKeyCustody::PassphraseWrapped);
    assert_eq!(status.store_revision, 1);
    // Load pass, then one write and one read-back of the target slot.
    assert_eq!(
        leaf.persist.io.calls,
        std::vec![
            Call::EnsureDirs,
            Call::Read(Slot::A),
            Call::Read(Slot::B),
            Call::Write(Slot::A),
            Call::Read(Slot::A),
        ]
    );
    let record_bytes = bytes_of(&leaf, Slot::A);
    let record = parse(Slot::A, &record_bytes).unwrap();
    assert!(record.present);
    assert_eq!(record.revision, 1);
    assert_eq!(record.name, NAME);
    assert_eq!(record.public_key, cheap_public());
    assert!(leaf.persist.io.file(Slot::B).is_none());
}

#[test]
fn a_new_leaf_restores_the_key_and_signs_like_the_k5_wrapped_fixture() {
    // `ssh-keygen -a 16` fixture, passphrase `hunter42`: the same key as the
    // plain fixture, so the signature must equal the plain key's (the K5
    // end-to-end expectation).
    let admin = Actor::admin();
    let login = Actor::login();
    let mut leaf = fresh();
    import_blob(&mut leaf, &admin, b"enc", &id_ed25519_enc()).unwrap();

    let mut restarted = reboot(&leaf);
    let named = status(&mut restarted, &admin, b"enc").unwrap();
    assert_eq!(named.public_key, id_ed25519_public());
    assert_eq!(named.custody, KmsSshKeyCustody::PassphraseWrapped);
    assert_eq!(named.posture, KmsCustodyPosture::DevPosturePersistent);
    assert_eq!(named.store_revision, 1);
    stage_secret(&mut restarted, &login, id_ed25519_public(), PASSPHRASE).unwrap();
    let signed = sign(
        &mut restarted,
        &login,
        id_ed25519_public(),
        SESSION_ID,
        b"user",
    )
    .unwrap();

    let mut plain = SshLeaf::serving(NoPersist);
    import_blob(&mut plain, &admin, b"pk", &id_ed25519()).unwrap();
    let expected = sign(&mut plain, &login, id_ed25519_public(), SESSION_ID, b"user").unwrap();
    assert_eq!(signed.signature, expected.signature);
    assert_eq!(signed.message_sha256, expected.message_sha256);
}

#[test]
fn delete_writes_a_tombstone_and_a_reload_is_empty_at_revision_zero() {
    let admin = Actor::admin();
    let mut leaf = fresh();
    import_cheap(&mut leaf, &admin).unwrap();
    delete_key(&mut leaf, &admin, NAME, cheap_public()).unwrap();
    let tomb_bytes = bytes_of(&leaf, Slot::B);
    let tomb = parse(Slot::B, &tomb_bytes).unwrap();
    assert!(!tomb.present);
    assert_eq!(tomb.revision, 2);
    assert_eq!(tomb.previous, digest(&bytes_of(&leaf, Slot::A)));

    let mut restarted = reboot(&leaf);
    let query = leaf_readiness(&mut restarted, &admin);
    assert_eq!(query.readiness, KmsCapabilityReadiness::Ready);
    assert_eq!(query.key_count, 0);
    // Seam-bound, accepted discrepancy (D-4b.14 a): the leaf counts from 0
    // again, and the posture reads volatile for an empty store.
    assert_eq!(query.store_revision, 0);
    assert_eq!(query.posture, KmsCustodyPosture::DevPostureVolatile);
    assert_eq!(
        status(&mut restarted, &admin, NAME),
        Err(KmsErrorCode::SshKeyNotFound)
    );
}

#[test]
fn an_import_after_a_delete_chains_to_the_tombstone() {
    let admin = Actor::admin();
    let mut leaf = fresh();
    import_cheap(&mut leaf, &admin).unwrap();
    delete_key(&mut leaf, &admin, NAME, cheap_public()).unwrap();

    // After a restart on the tombstone the revision restarts at 1, but the
    // new record still chains onto the tombstone and wins on the next load.
    let mut restarted = reboot(&leaf);
    let status = import_cheap(&mut restarted, &admin).unwrap();
    assert_eq!(status.store_revision, 1);
    let tomb_bytes = bytes_of(&restarted, Slot::B);
    let new_bytes = bytes_of(&restarted, Slot::A);
    let new = parse(Slot::A, &new_bytes).unwrap();
    assert_eq!(new.revision, 1);
    assert_eq!(new.previous, digest(&tomb_bytes));

    let mut again = reboot(&restarted);
    let named = status_of(&mut again);
    assert_eq!(named.store_revision, 1);
    assert_eq!(named.public_key, cheap_public());
}

fn status_of(leaf: &mut FileLeaf) -> types::kms::SshKeyStatusPayload {
    status(leaf, &Actor::admin(), NAME).unwrap()
}

#[test]
fn a_repair_import_after_corrupt_serves_and_loads_back() {
    let admin = Actor::admin();
    for (a, b) in [
        (Some(&b"junk"[..]), None),
        (None, Some(&b"junk"[..])),
        (Some(&b"junk"[..]), Some(&b"more junk"[..])),
    ] {
        let mut io = MemBlobIo::new();
        io.dirs = true;
        if let Some(bytes) = a {
            io.put(Slot::A, bytes);
        }
        if let Some(bytes) = b {
            io.put(Slot::B, bytes);
        }
        let mut leaf = leaf_over(io);
        assert_eq!(
            leaf_readiness(&mut leaf, &admin).readiness,
            KmsCapabilityReadiness::ProviderError
        );
        let repaired = import_cheap(&mut leaf, &admin).unwrap();
        assert_eq!(repaired.store_revision, 1);
        assert_eq!(leaf.state, LeafState::Serving);
        assert_eq!(
            leaf_readiness(&mut leaf, &admin).readiness,
            KmsCapabilityReadiness::Ready
        );
        // The repair record is in A, chained to nothing.
        assert_eq!(
            parse(Slot::A, &bytes_of(&leaf, Slot::A)).unwrap().previous,
            [0; 32]
        );
        let mut restarted = reboot(&leaf);
        let named = status_of(&mut restarted);
        assert_eq!(named.store_revision, 1);
        assert_eq!(named.public_key, cheap_public());
    }
}

#[test]
fn a_repair_import_after_rollback_detection_chains_to_the_newer_record() {
    let admin = Actor::admin();
    let a = Raw::present(Slot::A).bytes();
    let mut b = Raw::present(Slot::B);
    b.revision = 7;
    let b = b.bytes();
    let mut io = MemBlobIo::new();
    io.put(Slot::A, &a);
    io.put(Slot::B, &b);
    let mut leaf = leaf_over(io);
    assert_eq!(
        leaf_readiness(&mut leaf, &admin).readiness,
        KmsCapabilityReadiness::PolicyMismatch
    );
    let repaired = import_cheap(&mut leaf, &admin).unwrap();
    assert_eq!(repaired.store_revision, 1);
    assert_eq!(leaf.state, LeafState::Serving);
    let new_bytes = bytes_of(&leaf, Slot::A);
    let new = parse(Slot::A, &new_bytes).unwrap();
    assert_eq!(new.previous, digest(&b));
    // B was never touched.
    assert_eq!(bytes_of(&leaf, Slot::B), b);

    let mut restarted = reboot(&leaf);
    let named = status_of(&mut restarted);
    assert_eq!(named.store_revision, 1);
    assert_eq!(restarted.state, LeafState::Serving);
}

#[test]
fn an_io_error_then_success_on_the_next_opcode() {
    let admin = Actor::admin();
    let mut io = MemBlobIo::new();
    io.fail_ensure_dirs = true;
    let mut leaf = leaf_over(io);
    let blob = cheap_blob();
    assert_eq!(
        begin_import(&mut leaf, &admin, NAME, blob.len() as u32, sha(&blob)).map(|_| ()),
        Err(KmsErrorCode::SshKeyUnavailable)
    );
    assert_eq!(leaf.state, LeafState::NotLoaded);
    leaf.persist.io.fail_ensure_dirs = false;
    // The next admitted opcode retries the load and the import proceeds.
    import_cheap(&mut leaf, &admin).unwrap();
    assert_eq!(leaf.state, LeafState::Serving);
}

#[test]
fn the_limiter_unlock_slot_floors_and_handles_do_not_survive_a_restart() {
    let admin = Actor::admin();
    let login = Actor::login();
    let mut leaf = fresh();
    import_cheap(&mut leaf, &admin).unwrap();
    // One failed unlock (wrong passphrase) bumps the counter; then a good
    // secret stays staged.
    stage_secret(&mut leaf, &login, cheap_public(), b"wrong").unwrap();
    assert_eq!(
        sign(&mut leaf, &login, cheap_public(), SESSION_ID, b"user"),
        Err(KmsErrorCode::SshUnlockFailed)
    );
    assert_eq!(leaf.failures, 1);
    stage_secret(&mut leaf, &login, cheap_public(), PASS).unwrap();
    assert!(!matches!(leaf.unlock, UnlockSlot::Empty));
    assert!(leaf.floors.floor(admin.principal()) > 0);
    assert!(leaf.handle_counter > 0);

    let mut restarted = reboot(&leaf);
    let _ = status_of(&mut restarted);
    assert_eq!(restarted.failures, 0);
    assert!(matches!(restarted.unlock, UnlockSlot::Empty));
    assert!(restarted.secret.iter().all(|byte| *byte == 0));
    assert_eq!(restarted.floors.floor(admin.principal()), 0);
    assert_eq!(restarted.floors.floor(login.principal()), 0);
    assert_eq!(restarted.handle_counter, 0);
    // The key itself did persist.
    assert_eq!(status_of(&mut restarted).public_key, cheap_public());
}

#[test]
fn the_wrapped_key_unlocks_and_signs_after_a_restart() {
    let admin = Actor::admin();
    let login = Actor::login();
    let mut leaf = fresh();
    import_cheap(&mut leaf, &admin).unwrap();
    let mut restarted = reboot(&leaf);
    stage_secret(&mut restarted, &login, cheap_public(), PASS).unwrap();
    let signed = sign(&mut restarted, &login, cheap_public(), SESSION_ID, b"user");
    assert!(signed.is_ok());
    // A wrong passphrase is refused after the restart too.
    stage_secret(&mut restarted, &login, cheap_public(), b"nope").unwrap();
    assert_eq!(
        sign(&mut restarted, &login, cheap_public(), SESSION_ID, b"user"),
        Err(KmsErrorCode::SshUnlockFailed)
    );
}
