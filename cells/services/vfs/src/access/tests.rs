use super::*;
use types::CellId;

const CELL: Caller = Caller {
    cell: CellId(5),
    generation: 1,
    sender_tid: 50,
    flags: api::caller_identity::CALLER_FLAG_VFS_MUTATE,
};

#[test]
fn reads_are_allowed_across_the_shipped_prefixes() {
    let table = AccessTable::new();
    for path in ["/bin/shell", "/data/x", "/tmp/x", "/mnt/sd/x", "/srv/x"] {
        assert!(table.can_read(CELL, path), "{path} should be readable");
    }
}

#[test]
fn a_path_matching_no_rule_is_denied_both_ways() {
    let table = AccessTable::new();
    // No leading slash → matches no prefix, not even "/".
    assert!(!table.can_read(CELL, "etc/shadow"));
    assert!(!table.can_write(CELL, "etc/shadow"));
    assert!(!table.can_read(CELL, ""));
}

#[test]
fn bin_is_readable_but_not_writable() {
    let table = AccessTable::new();
    assert!(table.can_read(CELL, "/bin/vfs"));
    assert!(!table.can_write(CELL, "/bin/vfs"));
}

#[test]
fn root_is_read_only() {
    let table = AccessTable::new();
    assert!(table.can_read(CELL, "/motd"));
    assert!(!table.can_write(CELL, "/motd"));
}

#[test]
fn a_whole_path_rule_overrides_the_prefix_it_sits_under() {
    static EXACT: &[PathRule] = &[PathRule {
        prefix: "/data/secret",
        allow_read_all: false,
        allow_write_all: false,
    }];
    let table = AccessTable::with_service_lookup(|_| None);
    let table = AccessTable {
        exact: EXACT,
        prefixes: rules::PREFIX_RULES,
        service_lookup: table.service_lookup,
    };
    assert!(!table.can_read(CELL, "/data/secret"));
    // Only the exact path is affected; its neighbours still follow /data/.
    assert!(table.can_read(CELL, "/data/secretive"));
    assert!(table.can_read(CELL, "/data/other"));
}

#[test]
fn non_mutator_cannot_write_any_path() {
    let unmutated = Caller {
        cell: CellId(5),
        generation: 1,
        sender_tid: 50,
        flags: 0,
    };
    let table = AccessTable::new();
    assert!(!table.can_write(unmutated, "/data/x"));
    assert!(!table.can_write(unmutated, "/tmp/x"));
    assert!(!table.can_write(unmutated, "/srv/x"));
    assert!(!table.can_remove_dir(unmutated, "/srv/dir"));
    assert!(!table.can_remove_tree(unmutated, "/srv/dir"));
}

#[test]
fn unflagged_caller_is_denied_mutation_regardless_of_path_policy() {
    let unflagged = Caller {
        cell: CellId(5),
        generation: 1,
        sender_tid: 50,
        flags: 0,
    };
    let table = AccessTable::new();
    assert!(table.can_read(unflagged, "/srv/test"));
    assert!(!table.can_write(unflagged, "/srv/test"));
    assert!(!table.can_remove_dir(unflagged, "/srv/test"));
    assert!(!table.can_remove_tree(unflagged, "/srv/test"));
}
