#![allow(dead_code)] // reason: write path wired in full VirtIO-FAT phase
//! Per-cell open file handle table for the VFS service.
//!
//! Maps a `CapId` (issued by the kernel) to the VFS-internal state needed to
//! service subsequent `Read`, `Write`, `Seek`, and `Close` IPC requests.

use alloc::collections::BTreeMap;
use api::cap::CapId;
use types::{CellId, VAddr};

/// State for one open file handle inside the VFS cell.
pub struct HandleEntry {
    /// The owning cell (for quota accounting).
    pub owner: CellId,
    /// Pointer into the in-memory data slice (RamFS backing).
    /// Zero for directories or write-mode files not yet flushed.
    pub data_ptr: VAddr,
    /// Length of the data slice.
    pub data_len: usize,
    /// Current read/write position within `data_ptr..data_ptr+data_len`.
    pub pos: usize,
    /// Whether this handle is open for writing.
    pub writable: bool,
}

/// VFS-internal file handle table.
#[derive(Default)]
pub struct HandleTable {
    entries: BTreeMap<u64, HandleEntry>,
}

impl HandleTable {
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    /// Register a new read-only handle backed by `data_ptr/data_len`.
    pub fn insert_ro(&mut self, cap: CapId, owner: CellId, data_ptr: VAddr, data_len: usize) {
        self.entries.insert(
            cap.0,
            HandleEntry {
                owner,
                data_ptr,
                data_len,
                pos: 0,
                writable: false,
            },
        );
    }

    /// Look up a handle, returning a mutable reference for read/seek.
    pub fn get_mut(&mut self, cap: CapId) -> Option<&mut HandleEntry> {
        self.entries.get_mut(&cap.0)
    }

    /// Remove and return a handle (for Close).
    pub fn remove(&mut self, cap: CapId) -> Option<HandleEntry> {
        self.entries.remove(&cap.0)
    }
}
