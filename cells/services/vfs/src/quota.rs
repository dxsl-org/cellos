//! Per-cell disk quota enforcement for the VFS service.
//!
//! Phase 13 tracks bytes-on-disk per `CellId` and rejects writes that would
//! push a cell over its quota.  Default quota is `DEFAULT_QUOTA_BYTES`.

//! ## Who gets credited on delete
//! Usage is charged to the cell that wrote a path, so it must be released to that
//! same cell — not to whoever deletes the file. Releasing to the deleter let any
//! cell mint quota for itself by deleting another cell's files, and left the real
//! writer permanently charged for bytes that are gone. [`QuotaTracker`] therefore
//! remembers the charged writer per path.
//!
//! Bound on that record: if two cells write the same path, the first writer stays
//! recorded and is credited for the whole file when it is deleted. That
//! misallocates between two cells that already share a file, but it preserves the
//! property that matters — **deleting a file never credits the deleter**.
//! The record is in-memory, so it does not survive a hot-swap of this cell; after
//! one, a delete credits nobody, which errs toward over-charging rather than
//! toward handing out free quota.

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use types::{CellId, ViError, ViResult};

/// Default per-cell quota: 32 MB.
#[allow(dead_code)]
const DEFAULT_QUOTA_BYTES: u64 = 32 * 1024 * 1024;

/// Quota every charged path costs on top of its bytes: one 4 KiB allocation
/// unit, the least a file or directory occupies on FAT or CellosFS. Without
/// it empty files and directories were free, and each still costs heap here
/// (a `writers` record, a RamFS node) and an entry on disk.
#[allow(dead_code)]
const ENTRY_CHARGE: u64 = 4096;

/// Per-cell quota and usage tracker.
///
/// Keyed by `CellId` alone, without the caller generation: a respawned service
/// inherits its predecessor's usage because the bytes are still on disk.
pub struct QuotaTracker {
    used: BTreeMap<u64, u64>,
    /// Path → the cell whose ledger that path's bytes were charged to.
    writers: BTreeMap<String, CellId>,
    limit: u64,
    /// What each recorded path costs beyond its bytes (see [`ENTRY_CHARGE`]).
    entry: u64,
}

impl QuotaTracker {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            used: BTreeMap::new(),
            writers: BTreeMap::new(),
            limit: DEFAULT_QUOTA_BYTES,
            entry: ENTRY_CHARGE,
        }
    }

    /// Create a tracker with a custom byte limit and per-entry charge.
    ///
    /// Used by `test-hooks` builds to exercise quota enforcement with a tiny
    /// limit — writing the full 32 MB production limit over the 512-byte IPC
    /// path would take ~67k messages, far too slow for a QEMU integration test.
    #[cfg(any(test, feature = "test-hooks"))]
    pub fn with_limits(limit: u64, entry: u64) -> Self {
        Self {
            used: BTreeMap::new(),
            writers: BTreeMap::new(),
            limit,
            entry,
        }
    }

    /// Quota one recorded path costs beyond its bytes.
    pub fn entry_charge(&self) -> u64 {
        self.entry
    }

    /// Check whether `owner` can afford `bytes` without exceeding the quota.
    ///
    /// Does not mutate state — use before attempting the write, then call `charge`
    /// only if the actual disk write succeeds.
    pub fn can_charge(&self, owner: CellId, bytes: u64) -> bool {
        let used = self.used.get(&owner.0).copied().unwrap_or(0);
        used.saturating_add(bytes) <= self.limit
    }

    /// Charge `bytes` to `owner`.  Returns `Err(PermissionDenied)` if quota exceeded.
    pub fn charge(&mut self, owner: CellId, bytes: u64) -> ViResult<()> {
        let used = self.used.entry(owner.0).or_insert(0);
        if *used + bytes > self.limit {
            return Err(ViError::PermissionDenied);
        }
        *used += bytes;
        Ok(())
    }

    /// Release `bytes` from `owner` (on file delete or close-after-write).
    ///
    /// Prefer [`Self::release_path`]: this releases from whichever cell is named,
    /// so a caller that passes the requesting cell instead of the charged one
    /// hands out quota that was never charged.
    pub fn release(&mut self, owner: CellId, bytes: u64) {
        if let Some(used) = self.used.get_mut(&owner.0) {
            *used = used.saturating_sub(bytes);
        }
    }

    /// Record `owner` as the cell charged for `path`, replacing any earlier
    /// record.
    ///
    /// For a write that replaces the whole file: the previous contents have just
    /// been released to the previous writer, so the new writer owns all the bytes.
    fn set_writer(&mut self, path: &str, owner: CellId) {
        self.writers.insert(path.to_string(), owner);
    }

    /// The cell charged for `path`, if VFS charged anyone.
    ///
    /// `None` for a file that predates this cell's current instance (seeded at
    /// boot, or written before a hot-swap) — nothing was charged for it here, so
    /// nothing may be released for it either.
    pub fn writer_of(&self, path: &str) -> Option<CellId> {
        self.writers.get(path).copied()
    }

    /// What `owner` must still afford to replace `path`'s `old_size` bytes with
    /// `new_size`. Only the cell charged for the old contents gets them back:
    /// anyone else pays the whole new file, entry charge included.
    pub fn replace_cost(&self, owner: CellId, path: &str, old_size: u64, new_size: u64) -> u64 {
        let refund = if self.writer_of(path) == Some(owner) {
            old_size.saturating_add(self.entry)
        } else {
            0
        };
        new_size.saturating_add(self.entry).saturating_sub(refund)
    }

    /// Whether `owner` can afford [`Self::replace_cost`].
    pub fn can_replace(&self, owner: CellId, path: &str, old_size: u64, new_size: u64) -> bool {
        self.can_charge(owner, self.replace_cost(owner, path, old_size, new_size))
    }

    /// Account a completed replacement: the old contents go back to whoever
    /// was charged for them, the new ones are charged to `owner`.
    pub fn commit_replace(&mut self, owner: CellId, path: &str, old_size: u64, new_size: u64) {
        self.release_path(path, old_size);
        let _ = self.charge(owner, new_size.saturating_add(self.entry));
        self.set_writer(path, owner);
    }

    /// What appending `bytes` to `path` costs: the bytes, plus the entry charge
    /// when no cell is charged for the path yet.
    pub fn append_cost(&self, path: &str, bytes: u64) -> u64 {
        match self.writer_of(path) {
            Some(_) => bytes,
            None => bytes.saturating_add(self.entry),
        }
    }

    /// Account a completed append. Earlier bytes stay charged where they were;
    /// see the module note on files written by more than one cell.
    pub fn commit_append(&mut self, owner: CellId, path: &str, bytes: u64) {
        let _ = self.charge(owner, self.append_cost(path, bytes));
        self.writers.entry(path.to_string()).or_insert(owner);
    }

    /// Account a directory `owner` just created: the entry charge alone.
    pub fn commit_entry(&mut self, owner: CellId, path: &str) {
        let _ = self.charge(owner, self.entry);
        self.set_writer(path, owner);
    }

    /// Move the records at and below `old` to the same places below `new`
    /// after a rename: nothing on disk was freed, so nothing is released.
    pub fn move_tree(&mut self, old: &str, new: &str) {
        let (old, new) = (old.trim_end_matches('/'), new.trim_end_matches('/'));
        let moved: alloc::vec::Vec<String> = self
            .writers
            .keys()
            .filter(|held| {
                held.strip_prefix(old)
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
            })
            .cloned()
            .collect();
        for path in moved {
            if let Some(owner) = self.writers.remove(&path) {
                self.set_writer(&alloc::format!("{new}{}", &path[old.len()..]), owner);
            }
        }
    }

    /// Release `bytes` for `path`, plus its entry charge, to the cell that was
    /// charged, and forget the record.
    ///
    /// A path with no recorded writer releases nothing. That is deliberate: the
    /// alternative — crediting the requesting cell — is the bug this replaces.
    pub fn release_path(&mut self, path: &str, bytes: u64) {
        if let Some(owner) = self.writers.remove(path) {
            self.release(owner, bytes.saturating_add(self.entry));
        }
    }

    /// Release the entry charge of every record at or below `root`, after the
    /// tree is gone. Bytes of files must already be released by path
    /// ([`Self::release_path`]); what remains here are directories and files
    /// below the depth the caller measured.
    pub fn release_tree(&mut self, root: &str) {
        let root = root.trim_end_matches('/');
        let doomed: alloc::vec::Vec<String> = self
            .writers
            .keys()
            .filter(|held| {
                held.strip_prefix(root)
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
            })
            .cloned()
            .collect();
        for path in doomed {
            self.release_path(&path, 0);
        }
    }

    /// Return bytes used by `owner`.
    #[allow(dead_code)] // reason: consumed by the quota-report shell builtin planned in M2.1 follow-up; exercised by test-hooks builds
    pub fn used(&self, owner: CellId) -> u64 {
        self.used.get(&owner.0).copied().unwrap_or(0)
    }
}
