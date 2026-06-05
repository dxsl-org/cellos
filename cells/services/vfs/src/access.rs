//! CellId-based path access control for the VFS service.
//!
//! Uses path-prefix rules rather than POSIX mode bits.  In a Single Address
//! Space OS, `CellId` is the only meaningful identity — no uid/gid needed.
//!
//! Phase 30 (ELF capability manifests) will inject per-cell rules at spawn time
//! by reading the `.ViCell_manifest` ELF section.  For now, rules are hardcoded
//! and all authenticated cells may read and write to all paths.

use types::CellId;

/// Access rule for a single path prefix.
pub struct PathRule {
    /// The path prefix this rule applies to (e.g., `/data/`, `/bin/`).
    prefix: &'static str,
    /// True if any cell may read from this prefix.
    pub allow_read_all: bool,
    /// True if any cell may write to this prefix.
    /// False means only cells in `write_allowlist` (or nobody if the list is empty).
    pub allow_write_all: bool,
}

/// Table of path-prefix access rules evaluated in order (first match wins).
pub struct AccessTable {
    rules: &'static [PathRule],
}

/// Default rules: all cells may read and write to data/tmp; bin is read-only.
static DEFAULT_RULES: &[PathRule] = &[
    PathRule { prefix: "/bin/",  allow_read_all: true,  allow_write_all: false },
    PathRule { prefix: "/data/", allow_read_all: true,  allow_write_all: true  },
    PathRule { prefix: "/tmp/",  allow_read_all: true,  allow_write_all: true  },
    PathRule { prefix: "/",      allow_read_all: true,  allow_write_all: false }, // root: read-only
];

impl AccessTable {
    /// Initialize with the default rules.
    pub fn new() -> Self {
        Self { rules: DEFAULT_RULES }
    }

    /// Check whether `cell` may write to `path`.
    ///
    /// Returns `false` if no matching rule is found (deny by default).
    pub fn can_write(&self, _cell: CellId, path: &str) -> bool {
        for rule in self.rules {
            if path.starts_with(rule.prefix) {
                return rule.allow_write_all;
            }
        }
        false // no matching rule → deny
    }

    /// Check whether `cell` may read from `path`.
    ///
    /// Returns `false` if no matching rule is found (deny by default).
    pub fn can_read(&self, _cell: CellId, path: &str) -> bool {
        for rule in self.rules {
            if path.starts_with(rule.prefix) {
                return rule.allow_read_all;
            }
        }
        false
    }
}

impl Default for AccessTable {
    fn default() -> Self { Self::new() }
}
