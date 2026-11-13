//! The machine-pin store namespace, `/srv/.cellos-machine/**` (ADR-0029 §3).
//!
//! It holds the class-P machine fingerprint pin that the `machine-pin` Cell
//! compares on every boot (ADR-0029 §3, Phase 13 amendment: the provider
//! trusted init launched under that signed identity; the kernel registers
//! nothing else as `MACHINE_PIN`). Only the live registered
//! `service::MACHINE_PIN` provider may read, stat, list, create or write
//! anything here, and a write still needs the mutate flag like any other; every other Cell is refused,
//! and no caller may remove the namespace or an ancestor of it. The record
//! carries no authority, and this rule stops in-system Cells only: a raw-disk
//! or offline attacker is out of scope.

use crate::caller::Caller;
use abi::syscall::service;

const MACHINE_PIN_NAMESPACE: &str = "/srv/.cellos-machine";
const MACHINE_PIN_PREFIX: &str = "/srv/.cellos-machine/";

/// Whether `prefix` names one of the two rule rows that guard the namespace.
pub(super) fn is_machine_pin_store_rule(prefix: &str) -> bool {
    matches!(prefix, MACHINE_PIN_NAMESPACE | MACHINE_PIN_PREFIX)
}

/// Whether `path` is the namespace root or lies below it.
fn is_machine_pin_namespace_path(path: &str) -> bool {
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
        flags: abi::caller_identity::CALLER_FLAG_VFS_MUTATE,
    };

    const OTHER: Caller = Caller {
        cell: CellId(8),
        generation: 1,
        sender_tid: 80,
        flags: abi::caller_identity::CALLER_FLAG_VFS_MUTATE,
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

    /// No mutate-flag exception: the live provider without the flag is refused
    /// every write, even inside its own namespace (Phase 13; the machine-pin
    /// Cell declares `VfsMutate`). Reads keep needing only the live binding.
    #[test]
    fn the_live_provider_still_needs_the_mutate_flag_to_write() {
        const UNFLAGGED_PROVIDER: Caller = Caller {
            flags: 0,
            ..PROVIDER
        };
        let live = live_table();
        for path in INSIDE {
            assert!(live.can_read(UNFLAGGED_PROVIDER, path), "{path}: read");
            assert!(!live.can_write(UNFLAGGED_PROVIDER, path), "{path}: write");
            assert!(!live.can_remove_tree(PROVIDER, path), "{path}: remove");
            assert!(!live.can_remove_dir(PROVIDER, path), "{path}: rmdir");
        }
    }
}
