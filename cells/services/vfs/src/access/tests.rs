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

#[test]
fn guest_disk_is_writable_only_by_live_hypervisor() {
    let hv_table = AccessTable::with_service_lookup(|service_id| {
        (service_id == api::hypervisor::HYPERVISOR_SERVICE_ID).then_some(50)
    });
    assert!(hv_table.can_read(CELL, "/mnt/sd/guest_disk.img"));
    assert!(hv_table.can_write(CELL, "/mnt/sd/guest_disk.img"));
    assert!(!hv_table.can_remove_tree(CELL, "/mnt/sd/guest_disk.img"));
    assert!(!hv_table.can_remove_dir(CELL, "/mnt/sd/guest_disk.img"));

    let other_table = AccessTable::with_service_lookup(|_| None);
    assert!(other_table.can_read(CELL, "/mnt/sd/guest_disk.img"));
    assert!(!other_table.can_write(CELL, "/mnt/sd/guest_disk.img"));
}

#[test]
fn guest_disk_ancestors_and_image_cannot_be_removed_recursively() {
    let hv_table = AccessTable::with_service_lookup(|service_id| {
        (service_id == api::hypervisor::HYPERVISOR_SERVICE_ID).then_some(50)
    });
    for protected in [
        "/",
        "/mnt",
        "/mnt/",
        "/mnt/sd",
        "/mnt/sd/",
        "/mnt/sd/guest_disk.img",
    ] {
        assert!(
            !hv_table.can_remove_tree(CELL, protected),
            "{protected} should not allow recursive tree removal"
        );
        assert!(
            !hv_table.can_remove_dir(CELL, protected),
            "{protected} should not allow directory removal"
        );
    }

    for allowed in ["/mnt/sd/other", "/mnt/sd/other/", "/data/tmp"] {
        assert!(
            hv_table.can_remove_tree(CELL, allowed),
            "{allowed} should allow recursive tree removal"
        );
        assert!(
            hv_table.can_remove_dir(CELL, allowed),
            "{allowed} should allow directory removal"
        );
    }
}

#[test]
fn guest_disk_path_helper_matches_only_canonical_image() {
    assert!(is_guest_disk_path("/mnt/sd/guest_disk.img"));
    assert!(!is_guest_disk_path("/mnt/sd/guest_disk.img/"));
    assert!(!is_guest_disk_path("/mnt/sd/other.img"));
    assert!(!is_guest_disk_path("/mnt/sd"));
    assert!(!is_guest_disk_path("/"));
}

#[test]
fn guest_disk_case_insensitive_fat_lookup_is_protected() {
    let other_table = AccessTable::with_service_lookup(|_| None);
    for variant in [
        "/mnt/sd/GUEST_DISK.IMG",
        "/mnt/sd/Guest_Disk.img",
        "/MNT/SD/guest_disk.img",
    ] {
        assert!(is_guest_disk_path(variant));
        assert!(contains_guest_disk(variant));
        assert!(other_table.can_read(CELL, variant));
        assert!(
            !other_table.can_write(CELL, variant),
            "{variant} must deny write for non-hypervisor"
        );
        assert!(!other_table.can_remove_tree(CELL, variant));
        assert!(!other_table.can_remove_dir(CELL, variant));
    }
}

#[test]
fn guest_disk_sfn_aliases_and_83_names_are_protected() {
    let other_table = AccessTable::with_service_lookup(|_| None);
    for alias in [
        "/mnt/sd/GUEST_~1.IMG",
        "/mnt/sd/guest_~1.img",
        "/mnt/sd/GUESTD~1.IMG",
        "/mnt/sd/guestd~1.img",
        "/mnt/sd/GUEST~1.IMG",
        "/mnt/sd/guest~1.img",
        "/mnt/sd/GUEST_~2.IMG",
        "/mnt/sd/guest.img",
        "/mnt/sd/GUEST.IMG",
        "/mnt/sd/guestdsk.img",
    ] {
        assert!(is_guest_disk_path(alias), "{alias} should match guest disk");
        assert!(contains_guest_disk(alias), "{alias} should be contained");
        assert!(other_table.can_read(CELL, alias));
        assert!(
            !other_table.can_write(CELL, alias),
            "{alias} must deny write for non-hypervisor"
        );
        assert!(!other_table.can_remove_tree(CELL, alias));
        assert!(!other_table.can_remove_dir(CELL, alias));
    }

    for unrelated in [
        "/mnt/sd/other.img",
        "/mnt/sd/myguest.img",
        "/mnt/sd/guest1.txt",
    ] {
        assert!(!is_guest_disk_path(unrelated));
        assert!(other_table.can_write(CELL, unrelated));
    }
}

#[test]
fn multibyte_utf8_paths_do_not_panic_and_resolve_safely() {
    let other_table = AccessTable::with_service_lookup(|_| None);
    for path in [
        "/mnt/sdé",
        "/mnt/sd/café",
        "/mnt/sd/日本語.img",
        "/mnt/sd/guest_disk.imgé",
    ] {
        assert!(!is_guest_disk_path(path));
        let _ = other_table.can_write(CELL, path);
        let _ = contains_guest_disk(path);
    }
}
