use crate::caller::Caller;
use api::syscall::service;

pub(super) fn is_kms_store_rule(prefix: &str) -> bool {
    matches!(prefix, "/srv/cellos/kms" | "/srv/cellos/kms/")
}

pub(super) fn is_kms_store_rule_path(path: &str) -> bool {
    path == "/srv/cellos/kms" || path.starts_with("/srv/cellos/kms/")
}

pub(super) fn contains_kms_store(path: &str) -> bool {
    path == "/srv/cellos"
        || path == "/srv/cellos/"
        || path == "/srv/cellos/kms"
        || path.starts_with("/srv/cellos/kms/")
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

    #[test]
    fn kms_store_denies_arbitrary_and_stale_callers() {
        let table = AccessTable::with_service_lookup(|_| Some(77));
        for caller in [
            Caller {
                cell: CellId(7),
                generation: 1,
                sender_tid: 70,
            },
            Caller {
                cell: CellId(7),
                generation: 0,
                sender_tid: 77,
            },
            Caller {
                cell: CellId(7),
                generation: 1,
                sender_tid: 0,
            },
        ] {
            assert!(!table.can_read(caller, "/srv/cellos/kms"));
            assert!(!table.can_read(caller, "/srv/cellos/kms/slot-a"));
            assert!(!table.can_write(caller, "/srv/cellos/kms"));
            assert!(!table.can_write(caller, "/srv/cellos/kms/slot-a"));
        }
    }

    #[test]
    fn kms_store_allows_only_live_kms_provider() {
        let caller = Caller {
            cell: CellId(13),
            generation: 2,
            sender_tid: 91,
        };
        let table = AccessTable::with_service_lookup(|service_id| {
            (service_id == service::KMS).then_some(91)
        });
        for path in ["/srv/cellos/kms", "/srv/cellos/kms/slot-a"] {
            assert!(table.can_read(caller, path), "{path} should be readable");
            assert!(table.can_write(caller, path), "{path} should be writable");
        }
    }

    #[test]
    fn kms_store_fails_closed_when_lookup_fails() {
        let table = AccessTable::with_service_lookup(|_| None);
        assert!(!table.can_read(CELL, "/srv/cellos/kms/slot-a"));
        assert!(!table.can_write(CELL, "/srv/cellos/kms/slot-a"));
    }

    #[test]
    fn fast_path_denies_kms_store_without_lookup() {
        let table = AccessTable::with_service_lookup(|_| panic!("no lookup in fast path"));
        assert!(!table.can_read_fast(CELL, "/srv/cellos/kms/slot-a"));
        assert!(table.can_read_fast(CELL, "/srv/other"));
    }

    #[test]
    fn recursive_delete_cannot_bypass_kms_store_prefix() {
        let table = AccessTable::with_service_lookup(|service_id| {
            (service_id == service::KMS).then_some(50)
        });
        assert!(!table.can_remove_tree(CELL, "/srv/cellos"));
        assert!(!table.can_remove_tree(CELL, "/srv/cellos/kms"));
        assert!(table.can_remove_tree(CELL, "/srv/other"));
    }
}
