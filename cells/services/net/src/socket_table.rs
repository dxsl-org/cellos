//! CapId-keyed socket handle table for the net service cell.
//!
//! Maps kernel-issued `CapId`s (u64) to smoltcp `SocketHandle`s so that
//! any consumer cell can reference an open socket across IPC calls without
//! exposing smoltcp-internal handles.

extern crate alloc;

use alloc::collections::BTreeMap;
use smoltcp::iface::SocketHandle;
use types::ViError;

/// Maximum simultaneous sockets (including the DHCP management socket).
pub const MAX_SOCKETS: usize = 18; // 16 user + 1 DHCP + 1 ARP

/// Maps a `CapId` to a smoltcp `SocketHandle`.
#[derive(Default)]
pub struct SocketTable {
    entries: BTreeMap<u64, SocketHandle>,
    next_cap: u64,
}

impl SocketTable {
    pub fn new() -> Self {
        Self { entries: BTreeMap::new(), next_cap: 1 }
    }

    /// Allocate a new `CapId` and associate it with `handle`.
    ///
    /// # Errors
    /// Returns `ViError::OutOfMemory` if `MAX_SOCKETS` is already reached.
    pub fn insert(&mut self, handle: SocketHandle) -> Result<u64, ViError> {
        if self.entries.len() >= MAX_SOCKETS {
            return Err(ViError::OutOfMemory);
        }
        let cap = self.next_cap;
        self.next_cap += 1;
        self.entries.insert(cap, handle);
        Ok(cap)
    }

    /// Look up the smoltcp `SocketHandle` for a given `CapId`.
    #[allow(dead_code)] // reason: used by send/recv data-path (Phase 17)
    pub fn get(&self, cap: u64) -> Option<SocketHandle> {
        self.entries.get(&cap).copied()
    }

    /// Remove a socket from the table (called on close).
    pub fn remove(&mut self, cap: u64) -> Option<SocketHandle> {
        self.entries.remove(&cap)
    }
}
