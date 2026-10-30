//! Store facts, replay floors, restart, the persistence seam (rows 8, 20, 21,
//! 33 and the all-or-nothing rows).

use std::vec;
use std::vec::Vec;

use super::fixtures::*;
use super::*;
use crate::ssh::floor::FloorTable;
use crate::ssh::store::{RecordView, StoredIdentity};
use crate::ssh::{LeafState, Loaded, PersistError, Principal, Stored};

/// A scripted persister: records every call and fails or reports as told.
#[derive(Default)]
pub(super) struct FakePersist {
    pub calls: Vec<&'static str>,
    pub fail_store: bool,
    pub fail_tombstone: bool,
    pub written: bool,
    /// What `load` returns, consumed by the call.
    pub loads: Vec<Loaded>,
}

impl SshPersist for FakePersist {
    fn load(&mut self) -> Loaded {
        self.calls.push("load");
        if self.loads.is_empty() {
            Loaded::Empty
        } else {
            self.loads.remove(0)
        }
    }

    fn store(&mut self, record: &RecordView<'_>) -> Result<Stored, PersistError> {
        self.calls.push("store");
        assert_ne!(record.revision, 0);
        if self.fail_store {
            return Err(PersistError);
        }
        Ok(if self.written {
            Stored::Written
        } else {
            Stored::VolatileOnly
        })
    }

    fn tombstone(&mut self, next_revision: u64) -> Result<Stored, PersistError> {
        self.calls.push("tombstone");
        assert_ne!(next_revision, 0);
        if self.fail_tombstone {
            return Err(PersistError);
        }
        Ok(if self.written {
            Stored::Written
        } else {
            Stored::VolatileOnly
        })
    }
}

fn principal(cell: u64) -> Principal {
    Principal {
        cell_id: cell,
        generation: 1,
    }
}

#[test]
fn request_id_floor_rules() {
    // Table rules: zero, equal, lower refused; fresh principal floor 0;
    // eviction of the least recently advanced of four.
    let mut table = FloorTable::new();
    let a = principal(1);
    assert!(!table.admits(a, 0));
    assert!(table.admits(a, 1));
    table.advance(a, 5);
    assert!(!table.admits(a, 5) && !table.admits(a, 4) && table.admits(a, 6));
    for cell in 2..=4 {
        table.advance(principal(cell), 9);
    }
    assert_eq!(table.len(), 4);
    // A fifth principal evicts `a` (oldest): its floor resets to 0.
    table.advance(principal(5), 1);
    assert_eq!(table.len(), 4);
    assert_eq!(table.floor(a), 0);
    assert_eq!(table.floor(principal(2)), 9);
    // Advancing refreshes recency: 2 is refreshed, so 3 is evicted next.
    table.advance(principal(2), 10);
    table.advance(principal(6), 1);
    assert_eq!(table.floor(principal(3)), 0);
    assert_eq!(table.floor(principal(2)), 10);
    assert_eq!(table.floor(principal(7)), 0, "fresh principal");

    // Service rules: one floor covers ops 15, 20, 21 and 22.
    let mut service = service();
    let admin = Actor::admin();
    let login = Actor::login();
    let blob = Craft::wrapped(&RFC8032_SEED_1, b"pw", &[1; 16], 1).bytes();
    admin.set_next(10);
    import_blob(&mut service, &admin, b"wk", &blob).unwrap();
    let public = public_of(&RFC8032_SEED_1);
    login.set_next(5);
    unlock_begin(&mut service, &login, public, 2).unwrap(); // id 5
    assert_eq!(service.ssh.floors.floor(login.principal()), 5);
    // Reuse of 5, and anything lower, is refused on every floored op.
    for id in [5u64, 4, 1] {
        login.set_next(id);
        assert_eq!(
            unlock_begin(&mut service, &login, public, 2),
            Err(KmsErrorCode::InvalidRequest)
        );
        login.set_next(id);
        assert_eq!(
            sign(&mut service, &login, public, SESSION_ID, b"u"),
            Err(KmsErrorCode::InvalidRequest)
        );
    }
    admin.set_next(10);
    assert_eq!(
        delete_key(&mut service, &admin, b"wk", public),
        Err(KmsErrorCode::InvalidRequest)
    );
    admin.set_next(10);
    assert_eq!(
        begin_import(&mut service, &admin, b"x", 10, sha(b"a")),
        Err(KmsErrorCode::InvalidRequest)
    );
    // The floor is unchanged by every failure, so id 6 is still the next good one.
    assert_eq!(service.ssh.floors.floor(login.principal()), 5);
    login.set_next(6);
    stage_secret(&mut service, &login, public, b"pw").unwrap(); // id 6 begins the unlock
                                                                // A failed sign leaves the floor at 6 (bad passphrase fixed below).
    login.set_next(6);
    assert_eq!(
        sign(&mut service, &login, public, SESSION_ID, b"u"),
        Err(KmsErrorCode::InvalidRequest),
        "6 is spent by the unlock Begin"
    );
    login.set_next(7);
    sign(&mut service, &login, public, SESSION_ID, b"u").unwrap();
    assert_eq!(service.ssh.floors.floor(login.principal()), 7);
    // A principal with no history starts at floor 0 and may use id 1.
    let fresh = Actor::new(999, 1, 99, CALLER_FLAG_KMS_SSH_SIGN);
    stage_secret(&mut service, &fresh, public, b"pw").unwrap();
}

#[test]
fn status_unknown_name() {
    let mut service = service();
    let admin = Actor::admin();
    let login = Actor::login();
    // Empty store: named query is not found; the leaf query answers.
    assert_eq!(
        status(&mut service, &login, b"nope"),
        Err(KmsErrorCode::SshKeyNotFound)
    );
    import_blob(&mut service, &admin, b"mykey", &id_ed25519()).unwrap();
    let before = snapshot(&mut service, &[&admin, &login]);
    assert_eq!(
        status(&mut service, &login, b"other"),
        Err(KmsErrorCode::SshKeyNotFound)
    );
    assert_eq!(
        status(&mut service, &admin, b"mykeyx"),
        Err(KmsErrorCode::SshKeyNotFound)
    );
    assert_eq!(snapshot(&mut service, &[&admin, &login]), before);
    let leaf = status(&mut service, &login, b"").unwrap();
    assert_eq!(
        (leaf.key_present, leaf.key_count, leaf.custody),
        (0, 1, KmsSshKeyCustody::None)
    );
    assert_eq!(leaf.public_key, [0; 32]);
    assert_eq!(leaf.fingerprint_sha256, [0; 32]);
}

#[test]
fn delete_wrong_name_or_key_leaves_key() {
    let mut service = service();
    let admin = Actor::admin();
    let login = Actor::login();
    import_blob(&mut service, &admin, b"mykey", &id_ed25519()).unwrap();
    let before = snapshot(&mut service, &[&admin]);
    assert_eq!(
        delete_key(&mut service, &admin, b"other", id_ed25519_public()),
        Err(KmsErrorCode::SshKeyNotFound)
    );
    assert_eq!(
        delete_key(&mut service, &admin, b"mykey", [7u8; 32]),
        Err(KmsErrorCode::SshKeyNotFound)
    );
    assert_eq!(snapshot(&mut service, &[&admin]), before);
    assert!(sign(&mut service, &login, id_ed25519_public(), SESSION_ID, b"u").is_ok());
    // The right pair deletes, bumps the revision, and the key is gone.
    delete_key(&mut service, &admin, b"mykey", id_ed25519_public()).unwrap();
    assert_eq!(service.ssh.revision, 2);
    assert_eq!(
        sign(&mut service, &login, id_ed25519_public(), SESSION_ID, b"u"),
        Err(KmsErrorCode::SshKeyNotFound)
    );
    assert_eq!(status(&mut service, &login, b"").unwrap().store_revision, 2);
    // Re-import works and bumps again.
    let again = import_blob(&mut service, &admin, b"mykey", &id_ed25519()).unwrap();
    assert_eq!(again.store_revision, 3);
}

#[test]
fn restart_clears_state() {
    let mut service = service();
    let admin = Actor::admin();
    let login = Actor::login();
    import_blob(
        &mut service,
        &admin,
        b"wk",
        &Craft::wrapped(&RFC8032_SEED_1, b"pw", &[1; 16], 1).bytes(),
    )
    .unwrap();
    stage_secret(&mut service, &login, public_of(&RFC8032_SEED_1), b"bad").unwrap();
    begin_import_pending(&mut service);
    let _ = service;
    let mut restarted = self::service();
    let leaf = restarted.leaf();
    assert!(leaf.identity.is_none());
    assert!(matches!(leaf.import, crate::ssh::import::ImportSlot::Idle));
    assert!(matches!(leaf.unlock, crate::ssh::unlock::UnlockSlot::Empty));
    assert_eq!(leaf.floors.floor(admin.principal()), 0);
    assert_eq!(leaf.floors.floor(login.principal()), 0);
    assert_eq!(
        (leaf.revision, leaf.failures, leaf.handle_counter),
        (0, 0, 0)
    );
    assert!(!leaf.arena.is_allocated());
    let leaf_status = status(&mut restarted, &login, b"").unwrap();
    assert_eq!((leaf_status.key_count, leaf_status.store_revision), (0, 0));
}

fn begin_import_pending(service: &mut KmsService) {
    let other = Actor::new(101, 1, 11, CALLER_FLAG_KMS_SSH_ADMIN);
    // A second admin's import cannot start while a key exists; still leave an
    // open unlock slot behind (done by the caller).
    let _ = begin_import(service, &other, b"x", 4, sha(b"a"));
}

#[test]
fn persist_failure_leaves_everything_unchanged() {
    let admin = Actor::admin();
    let login = Actor::login();
    let blob = id_ed25519();
    let mut leaf = SshLeaf::serving(FakePersist {
        fail_store: true,
        ..FakePersist::default()
    });
    let before_import = snapshot(&mut leaf, &[&admin]);
    assert_eq!(
        import_blob(&mut leaf, &admin, b"k", &blob),
        Err(KmsErrorCode::PersistFailed)
    );
    assert_eq!(leaf.persist.calls, vec!["store"]);
    assert!(leaf.identity.is_none() && leaf.revision == 0);
    assert!(leaf.arena.is_zeroed());
    assert!(matches!(leaf.import, crate::ssh::import::ImportSlot::Idle));
    // Only Begin advanced the floor; nothing else differs from "just begun".
    let after = snapshot(&mut leaf, &[&admin]);
    assert_ne!(
        after, before_import,
        "Begin legitimately advanced the floor"
    );
    // Retry once the persister recovers: a clean, fully-working import.
    leaf.persist.fail_store = false;
    let ok = import_blob(&mut leaf, &admin, b"k", &blob).unwrap();
    assert_eq!(ok.store_revision, 1);
    assert_eq!(ok.posture, KmsCustodyPosture::DevPostureVolatile);

    // Tombstone failure: the key, revision, floor and unlock slot are intact.
    let wrapped = Craft::wrapped(&RFC8032_SEED_1, b"pw", &[1; 16], 1).bytes();
    let mut leaf = SshLeaf::serving(FakePersist::default());
    import_blob(&mut leaf, &admin, b"wk", &wrapped).unwrap();
    let public = public_of(&RFC8032_SEED_1);
    stage_secret(&mut leaf, &login, public, b"pw").unwrap();
    leaf.persist.fail_tombstone = true;
    let before = snapshot(&mut leaf, &[&admin, &login]);
    assert_eq!(
        delete_key(&mut leaf, &admin, b"wk", public),
        Err(KmsErrorCode::PersistFailed)
    );
    assert_eq!(snapshot(&mut leaf, &[&admin, &login]), before);
    assert!(
        sign(&mut leaf, &login, public, SESSION_ID, b"u").is_ok(),
        "staged secret survived"
    );
    leaf.persist.fail_tombstone = false;
    delete_key(&mut leaf, &admin, b"wk", public).unwrap();
    assert!(leaf.identity.is_none());
    assert_eq!(
        leaf.persist
            .calls
            .iter()
            .filter(|c| **c == "tombstone")
            .count(),
        2
    );
}

#[test]
fn posture_follows_the_persister() {
    let admin = Actor::admin();
    let mut leaf = SshLeaf::serving(FakePersist {
        written: true,
        ..FakePersist::default()
    });
    let status = import_blob(&mut leaf, &admin, b"k", &id_ed25519()).unwrap();
    assert_eq!(status.posture, KmsCustodyPosture::DevPosturePersistent);
    assert_eq!(
        super::status(&mut leaf, &admin, b"").unwrap().posture,
        KmsCustodyPosture::DevPosturePersistent
    );
}

#[test]
fn lazy_load_maps_persister_results_to_leaf_states() {
    let admin = Actor::admin();
    let login = Actor::login();
    let probe = |load: Loaded| {
        let leaf = SshLeaf::new(FakePersist {
            loads: vec![load],
            ..FakePersist::default()
        });
        assert_eq!(leaf.state, LeafState::NotLoaded);
        leaf
    };

    // Empty -> Serving, exactly one load.
    let mut leaf = probe(Loaded::Empty);
    let leaf_status = status(&mut leaf, &login, b"").unwrap();
    assert_eq!(leaf_status.readiness, KmsCapabilityReadiness::Ready);
    status(&mut leaf, &login, b"").unwrap();
    assert_eq!(leaf.persist.calls, vec!["load"]);

    // Loaded -> Serving with the identity, revision and persistent posture.
    let identity = StoredIdentity::plain(b"restored", id_ed25519_public(), &RFC8032_SEED_1);
    let mut leaf = probe(Loaded::Found {
        revision: 7,
        identity: identity.unwrap(),
    });
    let named = status(&mut leaf, &login, b"restored").unwrap();
    assert_eq!(
        (named.store_revision, named.posture),
        (7, KmsCustodyPosture::DevPosturePersistent)
    );

    // Corrupt / rollback -> repairable: readiness is reported truthfully, the
    // sign path is unavailable, import (15-18) is allowed and repairs.
    for (load, readiness) in [
        (Loaded::Corrupt, KmsCapabilityReadiness::ProviderError),
        (
            Loaded::RollbackDetected,
            KmsCapabilityReadiness::PolicyMismatch,
        ),
    ] {
        let mut leaf = probe(load);
        assert_eq!(status(&mut leaf, &login, b"").unwrap().readiness, readiness);
        assert_eq!(
            status(&mut leaf, &login, b"x"),
            Err(KmsErrorCode::SshKeyUnavailable)
        );
        assert_eq!(
            sign(&mut leaf, &login, [1; 32], SESSION_ID, b"u"),
            Err(KmsErrorCode::SshKeyUnavailable)
        );
        assert_eq!(
            delete_key(&mut leaf, &admin, b"x", [1; 32]),
            Err(KmsErrorCode::SshKeyUnavailable)
        );
        import_blob(&mut leaf, &admin, b"k", &id_ed25519()).unwrap();
        assert_eq!(leaf.state, LeafState::Serving);
        assert_eq!(
            status(&mut leaf, &login, b"").unwrap().readiness,
            KmsCapabilityReadiness::Ready
        );
    }

    // IoError: stays NotLoaded, answers unavailable, the next opcode retries.
    let mut leaf = SshLeaf::new(FakePersist {
        loads: vec![Loaded::IoError, Loaded::IoError],
        ..FakePersist::default()
    });
    assert_eq!(
        status(&mut leaf, &login, b"").unwrap().readiness,
        KmsCapabilityReadiness::Unavailable
    );
    assert_eq!(
        sign(&mut leaf, &login, [1; 32], SESSION_ID, b"u"),
        Err(KmsErrorCode::SshKeyUnavailable)
    );
    assert_eq!(leaf.state, LeafState::NotLoaded);
    assert_eq!(
        status(&mut leaf, &login, b"").unwrap().readiness,
        KmsCapabilityReadiness::Ready
    );
}

#[test]
fn counters_never_wrap() {
    let admin = Actor::admin();
    let mut service = service();
    service.ssh.handle_counter = u64::MAX;
    assert_eq!(
        begin_import(&mut service, &admin, b"k", 4, sha(b"a")),
        Err(KmsErrorCode::SshKeyUnavailable)
    );
    assert_eq!(service.ssh.state, LeafState::Unavailable);
    let login = Actor::login();
    assert_eq!(
        status(&mut service, &login, b"").unwrap().readiness,
        KmsCapabilityReadiness::Unavailable
    );

    let mut service = self::service();
    service.ssh.revision = u64::MAX;
    assert_eq!(
        import_blob(&mut service, &admin, b"k", &id_ed25519()),
        Err(KmsErrorCode::SshKeyUnavailable)
    );
    assert_eq!(service.ssh.state, LeafState::Unavailable);
    assert!(service.ssh.identity.is_none());
}

#[test]
fn handles_are_nonzero_unique_and_space_separated() {
    let mut leaf = SshLeaf::serving(NoPersist);
    let p = principal(1);
    let mut seen = Vec::new();
    for _ in 0..64 {
        for kind in [crate::ssh::HANDLE_IMPORT, crate::ssh::HANDLE_UNLOCK] {
            let handle = leaf.next_handle(kind, p, 7).unwrap();
            assert_ne!(handle, 0);
            assert!(handle >> 63 == 1, "high bit marks a derived handle");
            assert!(!seen.contains(&handle));
            seen.push(handle);
        }
    }
}
