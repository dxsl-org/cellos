//! Policy-table tests for `/srv/sshd`. Stat, ListDir and every handle-addressed
//! read authorize through `can_read`; Mkdir, Write, Append, Unlink, Rename and
//! D03 save through `can_write`; Rmdir/RmdirRecursive through `can_remove_*`.

use super::*;
use crate::access::AccessTable;
use api::caller_identity::CALLER_FLAG_VFS_MUTATE;
use api::cell_image::{ImageIdentity, ANCHOR_PRIVATE};
use types::CellId;

pub(super) const SSHD: Caller = Caller {
    cell: CellId(21),
    generation: 4,
    sender_tid: 210,
    flags: CALLER_FLAG_VFS_MUTATE,
};
pub(super) const DEV_SSHD: Caller = Caller {
    cell: CellId(26),
    generation: 1,
    ..SSHD
};
/// Same `CellId` as `SSHD`, a later generation the kernel has no record for.
pub(super) const RESPAWNED: Caller = Caller {
    generation: 5,
    ..SSHD
};
pub(super) const UNSIGNED_SSHD: Caller = Caller {
    cell: CellId(22),
    generation: 1,
    ..SSHD
};
pub(super) const OTHER_PROGRAM: Caller = Caller {
    cell: CellId(23),
    generation: 1,
    ..SSHD
};
const UNMEASURED: Caller = Caller {
    cell: CellId(24),
    generation: 1,
    ..SSHD
};
const ECHO_MISMATCH: Caller = Caller {
    cell: CellId(25),
    generation: 1,
    ..SSHD
};
const PRIVATE_SSHD: Caller = Caller {
    cell: CellId(27),
    generation: 1,
    ..SSHD
};

fn signed(anchor: u8, name: &[u8]) -> ImageIdentity {
    ImageIdentity::signed([1; 32], anchor, name).expect("valid identity")
}

/// Stand-in for `ResolveCellImage`.
pub(super) fn lookup(cell: u64, generation: u64) -> Option<CellImage> {
    let identity = match (cell, generation) {
        (21, 4) => signed(ANCHOR_RELEASE, b"sshd"),
        (26, 1) => signed(ANCHOR_DEV, b"sshd"),
        // `measured_identity` drops the name of an image that did not verify.
        (22, 1) => ImageIdentity::unsigned([2; 32]),
        (23, 1) => signed(ANCHOR_RELEASE, b"httpd"),
        (25, 1) => {
            return Some(CellImage {
                cell_id: 21,
                generation: 4,
                identity: signed(ANCHOR_RELEASE, b"sshd"),
            })
        }
        (27, 1) => signed(ANCHOR_PRIVATE, b"sshd"),
        _ => return None,
    };
    Some(CellImage {
        cell_id: cell,
        generation,
        identity,
    })
}

fn table() -> AccessTable {
    AccessTable::with_lookups(|_| None, lookup)
}

const INSIDE: [&str; 5] = [
    "/srv/sshd",
    "/srv/sshd/",
    "/srv/sshd/host_ed25519.seed",
    "/srv/sshd/authorized_keys",
    "/srv/sshd/keys/extra",
];

#[test]
fn signed_sshd_reads_and_writes_its_namespace() {
    let table = table();
    for caller in [SSHD, DEV_SSHD] {
        for path in INSIDE {
            assert!(table.can_read(caller, path), "{path} should read");
            assert!(table.can_write(caller, path), "{path} should write");
        }
    }
}

#[test]
fn every_other_identity_is_refused() {
    let table = table();
    let zero = Caller {
        cell: CellId(0),
        ..SSHD
    };
    let others = [
        RESPAWNED,
        UNSIGNED_SSHD,
        OTHER_PROGRAM,
        UNMEASURED,
        ECHO_MISMATCH,
        PRIVATE_SSHD,
        zero,
    ];
    for caller in others {
        for path in INSIDE {
            assert!(!table.can_read(caller, path), "{caller:?} read {path}");
            assert!(!table.can_write(caller, path), "{caller:?} write {path}");
        }
    }
    // Without the kernel query (host default) nobody matches, sshd included.
    for path in INSIDE {
        assert!(!AccessTable::new().can_read(SSHD, path));
    }
}

#[test]
fn sshd_still_needs_vfs_mutate_to_write() {
    let read_only = Caller { flags: 0, ..SSHD };
    assert!(table().can_read(read_only, "/srv/sshd/authorized_keys"));
    assert!(!table().can_write(read_only, "/srv/sshd/authorized_keys"));
}

#[test]
fn fast_path_refuses_even_sshd() {
    for path in INSIDE {
        assert!(!table().can_read_fast(SSHD, path), "{path}");
    }
}

#[test]
fn nobody_removes_the_namespace_or_an_ancestor() {
    let table = table();
    for path in ["/srv", "/srv/", "/srv/sshd", "/srv/sshd/", "/srv/sshd/keys"] {
        for caller in [SSHD, OTHER_PROGRAM] {
            assert!(!table.can_remove_dir(caller, path), "rmdir {path}");
            assert!(!table.can_remove_tree(caller, path), "rmtree {path}");
        }
    }
}

#[test]
fn neighbours_are_unaffected_and_sshd_gains_nothing_elsewhere() {
    let table = table();
    for path in ["/srv/sshd2/x", "/srv/sshdx", "/srv/other/file"] {
        assert!(table.can_read(OTHER_PROGRAM, path), "{path}");
        assert!(table.can_write(OTHER_PROGRAM, path), "{path}");
    }
    assert!(!table.can_read(SSHD, "/srv/cellos/kms/slot-a"));
    assert!(!table.can_write(SSHD, "/srv/.cellos-machine/pin"));
}
