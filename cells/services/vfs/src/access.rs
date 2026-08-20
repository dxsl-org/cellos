//! Path authorization for the VFS service.
//!
//! Path-prefix rules rather than POSIX mode bits: in a Single Address Space OS a
//! cell is the only principal, so there is no uid/gid to carry. The rule data and
//! the reasoning behind its shape live in [`rules`].
//!
//! Deny by default. A path matching no rule is refused, and so is a caller whose
//! identity the kernel did not vouch for — that check happens at the request
//! boundary in `dispatch`, before a path is ever looked at, because a caller with
//! no identity has no business reaching a policy table at all.
//!
//! These rules are VFS-internal and deliberately NOT driven by the cell manifest
//! the kernel reads at spawn (`__ViCell_manifest`, source of `BlockIoCap` /
//! `NetworkCap` / `SpawnCap`). Per-cell VFS path grants are a separate concern.

mod kms;
mod rules;

use crate::caller::Caller;
use kms::{contains_kms_store, is_kms_store_rule, is_kms_store_rule_path, live_kms_matches};
pub use rules::PathRule;

/// Path rules, evaluated whole-path first and by prefix second.
pub struct AccessTable {
    exact: &'static [PathRule],
    prefixes: &'static [PathRule],
    service_lookup: fn(u16) -> Option<usize>,
}

impl AccessTable {
    /// Initialize with the shipped rule tables.
    pub fn new() -> Self {
        Self::with_service_lookup(default_service_lookup)
    }

    fn with_service_lookup(service_lookup: fn(u16) -> Option<usize>) -> Self {
        Self {
            exact: rules::EXACT_RULES,
            prefixes: rules::PREFIX_RULES,
            service_lookup,
        }
    }

    /// Whether `caller` may write to `path`.
    ///
    /// `/srv/cellos/kms` is narrower than the broad `/srv/` prefix: only the
    /// live KMS service instance may touch it.
    pub fn can_write(&self, caller: Caller, path: &str) -> bool {
        self.decide(caller, path, AccessKind::Write)
    }

    /// Whether `caller` may read `path`.
    ///
    /// Every read op in `dispatch` is gated on this. Returns `false` when no rule
    /// matches — a relative path, for instance, matches no prefix including `/`.
    pub fn can_read(&self, caller: Caller, path: &str) -> bool {
        self.decide(caller, path, AccessKind::Read)
    }

    /// Interrupt-disabled fast path: never performs service lookup.
    ///
    /// The protected KMS prefix is denied here so the ecall path remains the
    /// only place that can prove "live service::KMS" with a normal lookup.
    pub fn can_read_fast(&self, caller: Caller, path: &str) -> bool {
        if is_kms_store_rule_path(path) {
            return false;
        }
        self.decide(caller, path, AccessKind::Read)
    }

    /// Whether `caller` may recursively remove `path` and its descendants.
    pub fn can_remove_tree(&self, caller: Caller, path: &str) -> bool {
        self.can_write(caller, path) && !contains_kms_store(path)
    }

    fn decide(&self, caller: Caller, path: &str, kind: AccessKind) -> bool {
        match self.rule_for(path) {
            Some(rule) if is_kms_store_rule(rule.prefix) => {
                live_kms_matches(caller, self.service_lookup)
            }
            Some(rule) => match kind {
                AccessKind::Read => rule.allow_read_all,
                AccessKind::Write => rule.allow_write_all,
            },
            None => false,
        }
    }

    fn rule_for(&self, path: &str) -> Option<&'static PathRule> {
        self.exact
            .iter()
            .find(|rule| rule.prefix == path)
            .or_else(|| {
                self.prefixes
                    .iter()
                    .find(|rule| path.starts_with(rule.prefix))
            })
    }
}

impl Default for AccessTable {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy)]
enum AccessKind {
    Read,
    Write,
}

#[cfg(target_os = "none")]
fn default_service_lookup(service_id: u16) -> Option<usize> {
    ostd::syscall::sys_lookup_service(service_id)
}

#[cfg(not(target_os = "none"))]
fn default_service_lookup(_service_id: u16) -> Option<usize> {
    None
}

#[cfg(test)]
mod tests {
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
}
