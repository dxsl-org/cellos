//! CapId-keyed socket handle table for the net service cell.
//!
//! Maps kernel-issued `CapId`s (u64) to smoltcp `SocketHandle`s so that
//! any consumer cell can reference an open socket across IPC calls without
//! exposing smoltcp-internal handles.

extern crate alloc;

use crate::socket_state::SocketState;
use alloc::collections::{BTreeMap, BTreeSet};
use smoltcp::iface::SocketHandle;
use types::ViError;

/// Maximum simultaneous sockets (including the DHCP management socket).
pub const MAX_SOCKETS: usize = 18; // 16 user + 1 DHCP + 1 ARP

/// Attested owner of a socket capability: bound to CellId and cell generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SocketOwner {
    pub cell_id: u64,
    pub generation: u64,
}

/// Maps a `CapId` to a smoltcp `SocketHandle` and connection state.
#[derive(Default)]
pub struct SocketTable {
    entries: BTreeMap<u64, SocketHandle>,
    states: BTreeMap<u64, SocketState>,
    owners: BTreeMap<u64, SocketOwner>,
    /// Bound listen port per cap — needed so ACCEPT can renew the listener on
    /// the same port after the original socket transitions to Established.
    listen_ports: BTreeMap<u64, u16>,
    /// Tracks which caps hold UDP sockets so TCP-only opcodes can reject them
    /// before calling `sockets.get_mut::<tcp::Socket>()`, which panics on a
    /// wrong-type handle.
    udp_caps: BTreeSet<u64>,
    next_cap: u64,
}

impl SocketTable {
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            states: BTreeMap::new(),
            owners: BTreeMap::new(),
            listen_ports: BTreeMap::new(),
            udp_caps: BTreeSet::new(),
            next_cap: 1,
        }
    }

    /// Allocate a new `CapId` and associate it with `handle` and attested `owner`.
    ///
    /// # Errors
    /// Returns `ViError::OutOfMemory` if `MAX_SOCKETS` is already reached.
    pub fn insert(&mut self, handle: SocketHandle, owner: SocketOwner) -> Result<u64, ViError> {
        if self.entries.len() >= MAX_SOCKETS {
            return Err(ViError::OutOfMemory);
        }
        let cap = self.next_cap;
        self.next_cap += 1;
        self.entries.insert(cap, handle);
        self.states.insert(cap, SocketState::Created);
        self.owners.insert(cap, owner);
        Ok(cap)
    }

    /// Allocate a new `CapId` for `handle` with an explicit initial state and attested owner.
    ///
    /// Unlike `insert` (which defaults to `Created`), this sets `state` directly.
    /// Only call from ACCEPT — the handle is already Established and must be
    /// surfaced as `Connected` to the consumer.
    ///
    /// # Errors
    /// Returns `ViError::OutOfMemory` if `MAX_SOCKETS` is already reached.
    pub fn insert_with_state(
        &mut self,
        handle: SocketHandle,
        state: SocketState,
        owner: SocketOwner,
    ) -> Result<u64, ViError> {
        if self.entries.len() >= MAX_SOCKETS {
            return Err(ViError::OutOfMemory);
        }
        let cap = self.next_cap;
        self.next_cap += 1;
        self.entries.insert(cap, handle);
        self.states.insert(cap, state);
        self.owners.insert(cap, owner);
        Ok(cap)
    }

    /// Check whether `caller` is the recorded owner of `cap`.
    pub fn is_owner(&self, cap: u64, caller: SocketOwner) -> bool {
        self.owners.get(&cap).copied() == Some(caller)
    }

    /// Look up the smoltcp `SocketHandle` for a given `CapId`, checking caller ownership.
    pub fn get(&self, cap: u64, caller: SocketOwner) -> Option<SocketHandle> {
        if self.is_owner(cap, caller) {
            self.entries.get(&cap).copied()
        } else {
            None
        }
    }

    /// Read the connection state for `cap`, checking caller ownership.
    pub fn get_state(&self, cap: u64, caller: SocketOwner) -> Option<SocketState> {
        if self.is_owner(cap, caller) {
            self.states.get(&cap).copied()
        } else {
            None
        }
    }

    /// Update the connection state for `cap`.
    pub fn set_state(&mut self, cap: u64, state: SocketState) {
        if self.entries.contains_key(&cap) {
            self.states.insert(cap, state);
        }
    }

    /// Record the port a listening socket is bound to, so ACCEPT can renew it.
    pub fn set_listen_port(&mut self, cap: u64, port: u16) {
        if self.entries.contains_key(&cap) {
            self.listen_ports.insert(cap, port);
        }
    }

    /// Read the bound listen port for `cap`, checking caller ownership.
    pub fn get_listen_port(&self, cap: u64, caller: SocketOwner) -> Option<u16> {
        if self.is_owner(cap, caller) {
            self.listen_ports.get(&cap).copied()
        } else {
            None
        }
    }
    /// Repoint an existing cap at a new smoltcp handle.
    ///
    /// Used by ACCEPT to swap the exhausted listening handle for a fresh one
    /// without changing the cap the consumer holds.
    pub fn update_handle(&mut self, cap: u64, new_handle: SocketHandle) {
        if self.entries.contains_key(&cap) {
            self.entries.insert(cap, new_handle);
        }
    }

    /// Mark a cap as holding a UDP socket.
    ///
    /// TCP-only opcodes (CONNECT, SEND, RECV, etc.) check this to avoid calling
    /// `sockets.get_mut::<tcp::Socket>()`, which panics on a wrong-type handle.
    pub fn mark_udp(&mut self, cap: u64) {
        self.udp_caps.insert(cap);
    }

    /// Returns `true` if `cap` holds a UDP socket owned by `caller`.
    pub fn is_udp(&self, cap: u64, caller: SocketOwner) -> bool {
        self.is_owner(cap, caller) && self.udp_caps.contains(&cap)
    }

    /// Remove a socket from the table if requested by its registered owner.
    pub fn remove(&mut self, cap: u64, caller: SocketOwner) -> Option<SocketHandle> {
        if !self.is_owner(cap, caller) {
            return None;
        }
        self.remove_internal(cap)
    }

    /// Remove a socket unconditionally (for internal error cleanup within the net cell).
    pub fn remove_internal(&mut self, cap: u64) -> Option<SocketHandle> {
        self.owners.remove(&cap);
        self.states.remove(&cap);
        self.listen_ports.remove(&cap);
        self.udp_caps.remove(&cap);
        self.entries.remove(&cap)
    }

    #[cfg(test)]
    pub(crate) fn next_cap_for_test(&self) -> u64 {
        self.next_cap
    }
}

#[cfg(test)]
mod tests;
