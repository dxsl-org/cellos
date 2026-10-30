//! Boundaries: admission before any I/O, the persister's independence from the
//! relay journal and its lifecycle, and the production-exclusion structure.

use types::kms::KmsOpcode;

use super::mem_io::Call;
use super::*;

const SSH_OPS: [KmsOpcode; 10] = [
    KmsOpcode::BeginSshKeyImport,
    KmsOpcode::ImportSshKeyChunk,
    KmsOpcode::CommitSshKeyImport,
    KmsOpcode::AbortSshKeyImport,
    KmsOpcode::GetSshKeyStatus,
    KmsOpcode::DeleteSshKey,
    KmsOpcode::SignSshUserauth,
    KmsOpcode::BeginSshUnlock,
    KmsOpcode::SshUnlockChunk,
    KmsOpcode::AbortSshUnlock,
];

#[test]
fn unadmitted_callers_cause_zero_storage_calls() {
    let mut leaf = fresh();
    let none = Actor::new(300, 1, 30, 0);
    let both = Actor::new(
        301,
        1,
        31,
        CALLER_FLAG_KMS_SSH_ADMIN | CALLER_FLAG_KMS_SSH_SIGN,
    );
    let admin = Actor::admin();
    let login = Actor::login();
    for op in SSH_OPS {
        // Unattested: no kernel identity at all.
        assert!(leaf.handle(op, &[0; 8], 5, None).is_err(), "{op:?}");
        assert!(leaf.persist.io.calls.is_empty(), "unattested {op:?}");
        // Neither flag, both flags.
        for actor in [&none, &both] {
            assert!(leaf.raw(op, &[0; 8], actor).is_err(), "{op:?}");
            assert!(
                leaf.persist.io.calls.is_empty(),
                "flags {:#x} {op:?}",
                actor.flags
            );
        }
    }
    // The wrong class for the opcode (status is open to both classes).
    for op in [
        KmsOpcode::BeginSshKeyImport,
        KmsOpcode::ImportSshKeyChunk,
        KmsOpcode::CommitSshKeyImport,
        KmsOpcode::AbortSshKeyImport,
        KmsOpcode::DeleteSshKey,
    ] {
        assert_eq!(
            leaf.raw(op, &[0; 8], &login),
            Err(KmsErrorCode::PermissionDenied)
        );
    }
    for op in [
        KmsOpcode::SignSshUserauth,
        KmsOpcode::BeginSshUnlock,
        KmsOpcode::SshUnlockChunk,
        KmsOpcode::AbortSshUnlock,
    ] {
        assert_eq!(
            leaf.raw(op, &[0; 8], &admin),
            Err(KmsErrorCode::PermissionDenied)
        );
    }
    assert!(leaf.persist.io.calls.is_empty());
    assert_eq!(leaf.state, crate::ssh::LeafState::NotLoaded);
    // The first admitted opcode is what runs the load, once.
    let _ = leaf_readiness(&mut leaf, &admin);
    assert_eq!(
        leaf.persist.io.calls,
        std::vec![Call::EnsureDirs, Call::Read(Slot::A), Call::Read(Slot::B)]
    );
}

/// The persister sources, which must not name the relay journal, its
/// lifecycle or its provider boundary.
const SOURCES: [(&str, &str); 4] = [
    ("persist.rs", include_str!("../../../ssh/persist.rs")),
    (
        "persist/blob_io.rs",
        include_str!("../../../ssh/persist/blob_io.rs"),
    ),
    (
        "persist/devroot.rs",
        include_str!("../../../ssh/persist/devroot.rs"),
    ),
    (
        "persist/record.rs",
        include_str!("../../../ssh/persist/record.rs"),
    ),
];

#[test]
fn the_persister_does_not_reach_the_relay_journal_or_its_lifecycle() {
    for (name, source) in SOURCES {
        for forbidden in [
            "storage::",
            "lifecycle",
            "RelayLifecycle",
            "ProviderSlot",
            "StoreIo",
        ] {
            assert!(
                !source.contains(forbidden),
                "{name} names `{forbidden}`: the SSH persister must not touch the relay journal"
            );
        }
    }
}

#[test]
fn the_persist_tree_is_exactly_these_files() {
    // A new file under `ssh/persist/` must be added to SOURCES above.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ssh/persist");
    let mut names: std::vec::Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(
        names,
        ["blob_io.rs", "devroot.rs", "record.rs"].map(std::string::String::from)
    );
}

#[test]
fn persist_implies_custody_and_inherits_every_custody_guard() {
    // `ssh-dev-persist = ["ssh-dev-custody"]`, default off.
    const _: () = assert!(cfg!(feature = "ssh-dev-custody"));
    const _: () = assert!(cfg!(feature = "ssh-dev-persist"));
    let manifest = include_str!("../../../../Cargo.toml");
    assert!(manifest.contains("ssh-dev-persist = [\"ssh-dev-custody\"]"));
    assert!(manifest.contains("default = []"));
    // build.rs refuses ssh-dev-custody (hence persist) under CELLOS_PRODUCTION
    // without naming persist: the refusal is transitive.
    let build = include_str!("../../../../build.rs");
    assert!(build.contains("CARGO_FEATURE_SSH_DEV_CUSTODY"));
    assert!(build.contains("ssh-dev-custody is forbidden in production builds"));
    assert!(!build.contains("PERSIST"));
    // lib.rs: custody (hence persist) is excluded from the hardware relay, and
    // persist is restricted to x86_64 bare metal.
    let lib = include_str!("../../../lib.rs");
    assert!(
        lib.contains("all(feature = \"ssh-dev-custody\", feature = \"hardware-relay-provider\")")
    );
    assert!(lib.contains("feature = \"ssh-dev-persist\""));
    assert!(lib.contains("all(target_arch = \"x86_64\", target_os = \"none\")"));
    assert!(lib.contains("ssh-dev-persist is restricted to the x86_64 bare-metal development lane"));
}

#[test]
fn only_two_files_are_nameable() {
    // The trait has no path argument: the persister can only address a slot.
    assert_eq!(Slot::A.file_name(), "ssh-a.bin");
    assert_eq!(Slot::B.file_name(), "ssh-b.bin");
    // After a full cycle every call is one of the three trait methods.
    let admin = Actor::admin();
    let leaf = leaf_with_two_records(&admin);
    for call in &leaf.persist.io.calls {
        assert!(matches!(
            call,
            Call::EnsureDirs | Call::Read(_) | Call::Write(_)
        ));
    }
}
