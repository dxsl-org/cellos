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

mod rules;

use crate::caller::Caller;
pub use rules::PathRule;

/// Path rules, evaluated whole-path first and by prefix second.
pub struct AccessTable {
    exact: &'static [PathRule],
    prefixes: &'static [PathRule],
}

impl AccessTable {
    /// Initialize with the shipped rule tables.
    pub fn new() -> Self {
        Self {
            exact: rules::EXACT_RULES,
            prefixes: rules::PREFIX_RULES,
        }
    }

    /// Whether `caller` may write to `path`.
    ///
    /// `caller` is accepted for every decision so that adding a cell-scoped rule
    /// is a change to [`rules`] alone; no rule discriminates on it today (see the
    /// [`rules`] module note).
    pub fn can_write(&self, _caller: Caller, path: &str) -> bool {
        self.decide(path)
            .map(|r| r.allow_write_all)
            .unwrap_or(false)
    }

    /// Whether `caller` may read `path`.
    ///
    /// Every read op in `dispatch` is gated on this. Returns `false` when no rule
    /// matches — a relative path, for instance, matches no prefix including `/`.
    pub fn can_read(&self, _caller: Caller, path: &str) -> bool {
        self.decide(path).map(|r| r.allow_read_all).unwrap_or(false)
    }

    /// The rule governing `path`: its whole-path rule if it has one, otherwise the
    /// first prefix rule it matches, otherwise nothing (deny).
    fn decide(&self, path: &str) -> Option<&'static PathRule> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use types::CellId;

    const CELL: Caller = Caller {
        cell: CellId(5),
        generation: 1,
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
        let table = AccessTable {
            exact: EXACT,
            prefixes: rules::PREFIX_RULES,
        };
        assert!(!table.can_read(CELL, "/data/secret"));
        // Only the exact path is affected; its neighbours still follow /data/.
        assert!(table.can_read(CELL, "/data/secretive"));
        assert!(table.can_read(CELL, "/data/other"));
    }
}
