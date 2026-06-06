//! Per-cell disk quota enforcement for the VFS service.
//!
//! Phase 13 tracks bytes-on-disk per `CellId` and rejects writes that would
//! push a cell over its quota.  Default quota is `DEFAULT_QUOTA_BYTES`.

use alloc::collections::BTreeMap;
use types::{CellId, ViError, ViResult};

/// Default per-cell quota: 32 MB.
const DEFAULT_QUOTA_BYTES: u64 = 32 * 1024 * 1024;

/// Per-cell quota and usage tracker.
#[derive(Default)]
pub struct QuotaTracker {
    used: BTreeMap<u64, u64>,
    limit: u64,
}

impl QuotaTracker {
    pub fn new() -> Self {
        Self { used: BTreeMap::new(), limit: DEFAULT_QUOTA_BYTES }
    }

    /// Create a tracker with a custom byte limit.
    ///
    /// Used by `test-hooks` builds to exercise quota enforcement with a tiny
    /// limit — writing the full 32 MB production limit over the 512-byte IPC
    /// path would take ~67k messages, far too slow for a QEMU integration test.
    #[cfg(feature = "test-hooks")]
    pub fn with_limit(limit: u64) -> Self {
        Self { used: BTreeMap::new(), limit }
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
    pub fn release(&mut self, owner: CellId, bytes: u64) {
        if let Some(used) = self.used.get_mut(&owner.0) {
            *used = used.saturating_sub(bytes);
        }
    }

    /// Return bytes used by `owner`.
    pub fn used(&self, owner: CellId) -> u64 {
        self.used.get(&owner.0).copied().unwrap_or(0)
    }

    /// Number of cells with recorded usage (for state-transfer sizing).
    pub fn entry_count(&self) -> usize { self.used.len() }

    /// Return all (cell_id, bytes_used) pairs for serialisation.
    pub fn all_entries(&self) -> alloc::vec::Vec<(u64, u64)> {
        self.used.iter().map(|(&k, &v)| (k, v)).collect()
    }

    /// Restore a previously serialised usage entry (called during hot-swap deserialise).
    pub fn restore(&mut self, owner: CellId, bytes: u64) {
        self.used.insert(owner.0, bytes);
    }
}
