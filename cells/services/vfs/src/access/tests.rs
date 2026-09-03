use super::*;
use types::CellId;

const CELL: Caller = Caller {
    cell: CellId(5),
    generation: 1,
    sender_tid: 50,
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
