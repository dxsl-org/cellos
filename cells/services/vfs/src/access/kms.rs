use crate::caller::Caller;
use api::syscall::service;

const KMS_NAMESPACE: &str = "/srv/cellos";

pub(super) fn is_kms_store_rule(prefix: &str) -> bool {
    matches!(prefix, KMS_NAMESPACE | "/srv/cellos/")
}

pub(super) fn is_kms_namespace_path(path: &str) -> bool {
    path == KMS_NAMESPACE || path.starts_with("/srv/cellos/")
}

pub(super) fn contains_kms_namespace(path: &str) -> bool {
    is_kms_namespace_path(path)
}

pub(super) fn is_canonical_policy_path(path: &str) -> bool {
    if path == "/" {
        return true;
    }
    if !path.starts_with('/') || path.ends_with("//") {
        return false;
    }
    let mut saw_component = false;
    for (index, component) in path.split('/').enumerate() {
        if index == 0 {
            continue;
        }
        if component.is_empty() {
            return index + 1 == path.split('/').count() && saw_component;
        }
        if matches!(component, "." | "..") {
            return false;
        }
        saw_component = true;
    }
    saw_component
}

pub(super) fn live_kms_matches(caller: Caller, lookup: fn(u16) -> Option<usize>) -> bool {
    if caller.cell.0 == 0 || caller.generation == 0 || caller.sender_tid == 0 {
        return false;
    }
    usize::try_from(caller.sender_tid)
        .ok()
        .and_then(|sender| lookup(service::KMS).filter(|live_tid| *live_tid == sender))
        .is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access::AccessTable;
    use types::CellId;

    const CELL: Caller = Caller {
        cell: CellId(5),
        generation: 1,
        sender_tid: 50,
    };

    fn live_table() -> AccessTable {
        AccessTable::with_service_lookup(|service_id| (service_id == service::KMS).then_some(50))
    }

    #[test]
    fn canonical_policy_path_rules_are_strict() {
        for path in [
            "",
            "srv/cellos",
            "/srv//cellos",
            "/srv/./cellos",
            "/srv/../cellos",
            "/srv/cellos//kms",
        ] {
            assert!(
                !is_canonical_policy_path(path),
                "{path} should be noncanonical"
            );
        }
        for path in ["/", "/srv", "/srv/", "/srv/cellos", "/srv/cellos/kms/"] {
            assert!(
                is_canonical_policy_path(path),
                "{path} should stay canonical"
            );
        }
    }

    #[test]
    fn namespace_is_reserved_to_live_kms_only() {
        let table = live_table();
        for path in ["/srv/cellos", "/srv/cellos/", "/srv/cellos/kms/slot-a"] {
            assert!(
                table.can_read(CELL, path),
                "{path} should read for live KMS"
            );
            assert!(
                table.can_write(CELL, path),
                "{path} should write for live KMS"
            );
        }
        let stale = AccessTable::with_service_lookup(|_| None);
        for path in ["/srv/cellos", "/srv/cellos/kms/slot-a"] {
            assert!(!stale.can_read(CELL, path), "{path} should deny stale read");
            assert!(
                !stale.can_write(CELL, path),
                "{path} should deny stale write"
            );
            assert!(
                !stale.can_read_fast(CELL, path),
                "{path} should deny fast path"
            );
            assert!(
                !stale.can_remove_tree(CELL, path),
                "{path} should deny remove"
            );
            assert!(
                !stale.can_remove_dir(CELL, path),
                "{path} should deny directory remove"
            );
        }
    }

    #[test]
    fn alias_paths_fail_before_rule_selection() {
        let table = live_table();
        for path in [
            "/srv/cellos//kms/slot-a",
            "/srv//cellos/kms/slot-a",
            "/srv/./cellos",
            "/srv/cellos/../kms",
        ] {
            assert!(!table.can_read(CELL, path), "{path} should not read");
            assert!(!table.can_write(CELL, path), "{path} should not write");
            assert!(
                !table.can_read_fast(CELL, path),
                "{path} should not fast-read"
            );
            assert!(
                !table.can_remove_tree(CELL, path),
                "{path} should not remove"
            );
            assert!(!table.can_remove_dir(CELL, path), "{path} should not rmdir");
        }
        assert!(table.can_read(CELL, "/srv/other"));
        assert!(table.can_write(CELL, "/srv/other"));
        assert!(table.can_remove_tree(CELL, "/srv/other"));
        assert!(table.can_remove_dir(CELL, "/srv/other"));
    }

    #[test]
    fn directory_removal_cannot_remove_kms_namespace() {
        let table = live_table();
        for path in ["/srv/cellos", "/srv/cellos/", "/srv/cellos/kms"] {
            assert!(!table.can_remove_dir(CELL, path), "{path} should not rmdir");
            assert!(
                !table.can_remove_tree(CELL, path),
                "{path} should not remove"
            );
        }
    }
}
