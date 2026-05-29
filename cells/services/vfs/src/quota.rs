#![allow(dead_code)] // reason: write path wired in full VirtIO-FAT phase
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
}
