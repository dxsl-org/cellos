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
/// Per-cell quota and usage tracker.
///
/// Keyed by `CellId` alone, without the caller generation: a respawned service
/// inherits its predecessor's usage because the bytes are still on disk.
#[derive(Default)]
pub struct QuotaTracker {
    used: BTreeMap<u64, u64>,
    /// Path → the cell whose ledger that path's bytes were charged to.
    writers: BTreeMap<String, CellId>,
    limit: u64,
}

impl QuotaTracker {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            used: BTreeMap::new(),
            writers: BTreeMap::new(),
            limit: DEFAULT_QUOTA_BYTES,
        }
    }

    /// Create a tracker with a custom byte limit.
    ///
    /// Used by `test-hooks` builds to exercise quota enforcement with a tiny
    /// limit — writing the full 32 MB production limit over the 512-byte IPC
    /// path would take ~67k messages, far too slow for a QEMU integration test.
    #[cfg(feature = "test-hooks")]
    pub fn with_limit(limit: u64) -> Self {
        Self {
            used: BTreeMap::new(),
            writers: BTreeMap::new(),
            limit,
        }
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
    pub fn set_writer(&mut self, path: &str, owner: CellId) {
        self.writers.insert(path.to_string(), owner);
    }

    /// Record `owner` as the cell charged for `path` only if no cell is recorded
    /// yet.
    ///
    /// For an append, which leaves earlier bytes charged where they were. See the
    /// module note on files written by more than one cell.
    pub fn record_writer(&mut self, path: &str, owner: CellId) {
        self.writers.entry(path.to_string()).or_insert(owner);
    }

    /// The cell charged for `path`, if VFS charged anyone.
    ///
    /// `None` for a file that predates this cell's current instance (seeded at
    /// boot, or written before a hot-swap) — nothing was charged for it here, so
    /// nothing may be released for it either.
    pub fn writer_of(&self, path: &str) -> Option<CellId> {
        self.writers.get(path).copied()
    }

    /// Release `bytes` for `path` from the cell that was charged, and forget the
    /// record.
    ///
    /// A path with no recorded writer releases nothing. That is deliberate: the
    /// alternative — crediting the requesting cell — is the bug this replaces.
    pub fn release_path(&mut self, path: &str, bytes: u64) {
        if let Some(owner) = self.writers.remove(path) {
            self.release(owner, bytes);
        }
    }

    /// Return bytes used by `owner`.
    #[allow(dead_code)] // reason: consumed by the quota-report shell builtin planned in M2.1 follow-up; exercised by test-hooks builds
    pub fn used(&self, owner: CellId) -> u64 {
        self.used.get(&owner.0).copied().unwrap_or(0)
    }

    /// Number of cells with recorded usage (for state-transfer sizing).
    pub fn entry_count(&self) -> usize {
        self.used.len()
    }

    /// Return all (cell_id, bytes_used) pairs for serialisation.
    pub fn all_entries(&self) -> alloc::vec::Vec<(u64, u64)> {
        self.used.iter().map(|(&k, &v)| (k, v)).collect()
    }

    /// Restore a previously serialised usage entry (called during hot-swap deserialise).
    pub fn restore(&mut self, owner: CellId, bytes: u64) {
        self.used.insert(owner.0, bytes);
    }
}
