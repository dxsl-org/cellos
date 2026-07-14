//! Pending async read table for the VFS service.
//!
//! Implements the server side of the two-opcode non-blocking read protocol:
//!   1. `ReadAsync { path }` → VFS reads file data synchronously (disk is still
//!      blocking), stores it under a handle, returns `PendingHandle(id)`.
//!   2. `Poll { handle: id }` → returns `Data(bytes)` (always ready with
//!      synchronous backend) or `Err` if the handle is stale/consumed.
//!
//! The protocol is correct regardless of the backend being synchronous: the
//! caller-side loop with `yield_now()` cooperates correctly with the scheduler,
//! and the API shape is ready for a future interrupt-driven block driver.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// One pending async read slot.
pub struct PendingRead {
    /// Pre-read file contents.  Data is available immediately with the current
    /// synchronous VirtIO block backend.
    pub data: Vec<u8>,
}

/// Table of pending reads keyed by opaque handle ID.
pub struct PendingTable {
    slots: BTreeMap<u32, PendingRead>,
    next_id: u32,
}

impl PendingTable {
    pub fn new() -> Self {
        Self {
            slots: BTreeMap::new(),
            next_id: 1,
        }
    }

    /// Insert pre-read data and return the handle.
    pub fn insert(&mut self, data: Vec<u8>) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1); // skip 0
        self.slots.insert(id, PendingRead { data });
        id
    }

    /// Consume the data for `handle` — returns `None` if the handle is
    /// stale (already polled) or was never issued.
    pub fn poll(&mut self, handle: u32) -> Option<Vec<u8>> {
        self.slots.remove(&handle).map(|p| p.data)
    }
}

impl Default for PendingTable {
    fn default() -> Self {
        Self::new()
    }
}
