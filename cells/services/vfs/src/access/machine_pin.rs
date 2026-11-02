//! The machine-pin store namespace, `/srv/.cellos-machine/**` (ADR-0029 §3).
//!
//! It holds the class-P machine fingerprint pin that trusted init compares on
//! every boot. Only the live registered `service::MACHINE_PIN` provider may
//! read, stat, list, create or write anything here; every other Cell is refused,
//! and no caller may remove the namespace or an ancestor of it. The record
//! carries no authority, and this rule stops in-system Cells only: a raw-disk
//! or offline attacker is out of scope.

use crate::caller::Caller;
use api::syscall::service;

const MACHINE_PIN_NAMESPACE: &str = "/srv/.cellos-machine";
const MACHINE_PIN_PREFIX: &str = "/srv/.cellos-machine/";

/// Whether `prefix` names one of the two rule rows that guard the namespace.
pub(super) fn is_machine_pin_store_rule(prefix: &str) -> bool {
    matches!(prefix, MACHINE_PIN_NAMESPACE | MACHINE_PIN_PREFIX)
}

/// Whether `path` is the namespace root or lies below it.
pub(super) fn is_machine_pin_namespace_path(path: &str) -> bool {
    path == MACHINE_PIN_NAMESPACE || path.starts_with(MACHINE_PIN_PREFIX)
}

/// Whether removing `path` would remove the namespace: `path` is inside it or
/// is one of its ancestors.
pub(super) fn contains_machine_pin_namespace(path: &str) -> bool {
    if is_machine_pin_namespace_path(path) {
        return true;
    }
    let ancestor = path.strip_suffix('/').unwrap_or(path);
    MACHINE_PIN_NAMESPACE
        .strip_prefix(ancestor)
        .is_some_and(|remainder| remainder.starts_with('/'))
}

/// Whether `caller` is the live `MACHINE_PIN` provider: an attested, non-zero
/// identity whose sending thread is the tid currently registered for the
/// service. A provider that died or was replaced stops matching at once.
pub(super) fn live_machine_pin_matches(caller: Caller, lookup: fn(u16) -> Option<usize>) -> bool {
    if caller.cell.0 == 0 || caller.generation == 0 || caller.sender_tid == 0 {
        return false;
    }
    usize::try_from(caller.sender_tid)
        .ok()
        .and_then(|sender| lookup(service::MACHINE_PIN).filter(|live_tid| *live_tid == sender))
        .is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access::AccessTable;
    use types::CellId;

    const PROVIDER: Caller = Caller {
        cell: CellId(7),
        generation: 1,
        sender_tid: 70,
        flags: api::caller_identity::CALLER_FLAG_VFS_MUTATE,
    };

    const OTHER: Caller = Caller {
        cell: CellId(8),
        generation: 1,
        sender_tid: 80,
        flags: api::caller_identity::CALLER_FLAG_VFS_MUTATE,
    };

    const INSIDE: [&str; 4] = [
        "/srv/.cellos-machine",
        "/srv/.cellos-machine/",
        "/srv/.cellos-machine/pin",
        "/srv/.cellos-machine/pin.tmp",
    ];

    fn live_table() -> AccessTable {
        AccessTable::with_service_lookup(|id| (id == service::MACHINE_PIN).then_some(70))
    }

    // Stat, ListDir and every handle-addressed read authorize through
    // `can_read`; Mkdir, Write, Append, Unlink, Rename and D03 save through
    // `can_write`.
    #[test]
    fn live_provider_reads_writes_stats_and_creates() {
        let table = live_table();
        for path in INSIDE {
            assert!(table.can_read(PROVIDER, path), "{path} should read");
            assert!(table.can_write(PROVIDER, path), "{path} should write");
        }
    }

    #[test]
    fn other_cells_and_stale_providers_are_refused() {
        let live = live_table();
        let unregistered = AccessTable::with_service_lookup(|_| None);
        let replaced =
            AccessTable::with_service_lookup(|id| (id == service::MACHINE_PIN).then_some(71));
        // The tid is live for a different service, not for MACHINE_PIN.
        let wrong_service =
            AccessTable::with_service_lookup(|id| (id == service::KMS).then_some(70));
        for path in INSIDE {
            assert!(!live.can_read(OTHER, path), "{path}: non-provider read");
            assert!(!live.can_write(OTHER, path), "{path}: non-provider write");
            for table in [&unregistered, &replaced, &wrong_service] {
                assert!(!table.can_read(PROVIDER, path), "{path}: stale read");
                assert!(!table.can_write(PROVIDER, path), "{path}: stale write");
            }
        }
        // The provider gains nothing in the KMS namespace either.
        assert!(!live.can_read(PROVIDER, "/srv/cellos/kms/slot-a"));
        assert!(!live.can_write(PROVIDER, "/srv/cellos/kms/slot-a"));
    }

    #[test]
    fn zero_identity_is_refused() {
        let table =
            AccessTable::with_service_lookup(|id| (id == service::MACHINE_PIN).then_some(0));
        let live = live_table();
        let no_cell = Caller {
            cell: CellId(0),
            ..PROVIDER
        };
        let no_generation = Caller {
            generation: 0,
            ..PROVIDER
        };
        let no_sender = Caller {
            sender_tid: 0,
            ..PROVIDER
        };
        for path in INSIDE {
            assert!(!live.can_read(no_cell, path), "{path}: zero cell");
            assert!(!live.can_write(no_cell, path), "{path}: zero cell");
            assert!(
                !live.can_read(no_generation, path),
                "{path}: zero generation"
            );
            assert!(
                !live.can_write(no_generation, path),
                "{path}: zero generation"
            );
            // A registry answering 0 must not admit a zero sender tid.
            assert!(!table.can_read(no_sender, path), "{path}: zero sender");
            assert!(!table.can_write(no_sender, path), "{path}: zero sender");
        }
    }

    #[test]
    fn fast_path_never_serves_the_namespace() {
        let table = live_table();
        for path in INSIDE {
            assert!(!table.can_read_fast(PROVIDER, path), "{path}: fast read");
            assert!(!table.can_read_fast(OTHER, path), "{path}: fast read");
        }
    }

    #[test]
    fn namespace_and_ancestors_cannot_be_removed() {
        let table = live_table();
        for path in [
            "/",
            "/srv",
            "/srv/",
            "/srv/.cellos-machine",
            "/srv/.cellos-machine/",
            "/srv/.cellos-machine/pin",
        ] {
            for caller in [PROVIDER, OTHER] {
                assert!(!table.can_remove_tree(caller, path), "{path}: remove tree");
                assert!(!table.can_remove_dir(caller, path), "{path}: rmdir");
            }
            assert!(contains_machine_pin_namespace(path), "{path}: contained");
        }
    }

    #[test]
    fn noncanonical_spellings_are_refused() {
        let table = live_table();
        for path in [
            "/srv/./.cellos-machine/pin",
            "/srv//.cellos-machine/pin",
            "/srv/x/../.cellos-machine/pin",
            "/srv/.cellos-machine//pin",
            "/srv/.cellos-machine/./pin",
        ] {
            for caller in [PROVIDER, OTHER] {
                assert!(!table.can_read(caller, path), "{path}: read");
                assert!(!table.can_write(caller, path), "{path}: write");
                assert!(!table.can_read_fast(caller, path), "{path}: fast read");
                assert!(!table.can_remove_tree(caller, path), "{path}: remove tree");
                assert!(!table.can_remove_dir(caller, path), "{path}: rmdir");
            }
        }
    }

    #[test]
    fn sibling_names_are_not_captured() {
        let table = live_table();
        for path in [
            "/srv/.cellos-machinex",
            "/srv/.cellos-machinex/pin",
            "/srv/.cellos-machin",
        ] {
            assert!(!is_machine_pin_namespace_path(path), "{path}: inside");
            assert!(!contains_machine_pin_namespace(path), "{path}: contained");
            assert!(table.can_read(OTHER, path), "{path}: ordinary read");
            assert!(table.can_write(OTHER, path), "{path}: ordinary write");
            assert!(
                table.can_remove_tree(OTHER, path),
                "{path}: ordinary remove"
            );
            assert!(table.can_remove_dir(OTHER, path), "{path}: ordinary rmdir");
        }
        for prefix in ["/srv/.cellos-machinex", "/srv/.cellos-machinex/", "/srv/"] {
            assert!(!is_machine_pin_store_rule(prefix), "{prefix}: rule");
        }
    }

    /// Init runs permit-all, so its trailer never carries the mutate flag. The
    /// exception admits exactly the live provider inside the namespace; every
    /// other caller and every other path still needs the flag.
    #[test]
    fn mutate_exception_covers_only_the_live_provider_inside_the_namespace() {
        const INIT: Caller = Caller {
            flags: 0,
            ..PROVIDER
        };
        const UNFLAGGED: Caller = Caller { flags: 0, ..OTHER };
        let live = live_table();
        for path in INSIDE {
            assert!(
                live.is_machine_pin_writer(INIT, path),
                "{path}: provider exception"
            );
            assert!(live.can_write(INIT, path), "{path}: provider write");
            assert!(
                !live.is_machine_pin_writer(UNFLAGGED, path),
                "{path}: other caller"
            );
            assert!(
                !live.can_write(UNFLAGGED, path),
                "{path}: other caller write"
            );
            assert!(!live.can_remove_tree(INIT, path), "{path}: provider remove");
            assert!(!live.can_remove_dir(INIT, path), "{path}: provider rmdir");
        }
        for table in [
            AccessTable::with_service_lookup(|_| None),
            AccessTable::with_service_lookup(|id| (id == service::MACHINE_PIN).then_some(71)),
        ] {
            assert!(
                !table.is_machine_pin_writer(INIT, "/srv/.cellos-machine/pin"),
                "stale provider"
            );
            assert!(
                !table.can_write(INIT, "/srv/.cellos-machine/pin"),
                "stale provider write"
            );
        }
        for path in [
            "/srv/notes",
            "/srv/.cellos-machinex/pin",
            "/srv/cellos/kms/slot-a",
            "/data/x",
            "/srv/./.cellos-machine/pin",
            "/srv//.cellos-machine/pin",
        ] {
            assert!(!live.is_machine_pin_writer(INIT, path), "{path}: outside");
            assert!(
                !live.can_write(INIT, path),
                "{path}: provider without flag outside"
            );
        }
    }
}
