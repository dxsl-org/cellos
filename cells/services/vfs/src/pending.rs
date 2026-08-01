//! Pending async read table for the VFS service.
//!
//! Implements the server side of the two-opcode non-blocking read protocol:
//!   1. `ReadAsync { path }` → VFS reads file data synchronously (disk is still
//!      blocking), stores it under a handle owned by the requesting cell, and
//!      returns `PendingHandle(id)`.
//!   2. `Poll { handle: id }` → returns `Data(bytes)` (always ready with
//!      synchronous backend) or `Err` if the handle is stale/consumed or
//!      belongs to another cell.
//!
//! The protocol is correct regardless of the backend being synchronous: the
//! caller-side loop with `yield_now()` cooperates correctly with the scheduler,
//! and the API shape is ready for a future interrupt-driven block driver.
//!
//! Handle IDs are allocated sequentially from 1 and are therefore trivially
//! guessable. They are not secrets: a slot's confidentiality rests entirely on
//! the owner comparison in [`PendingTable::poll`], never on a caller failing to
//! guess an ID.

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::caller::Caller;

/// One pending async read slot.
pub struct PendingRead {
    /// The caller that issued the `ReadAsync`, and the only caller permitted to
    /// poll this slot.  Carries the cell generation, so a cell that reappears
    /// under a recycled `CellId` cannot poll its predecessor's slots.
    pub owner: Caller,
    /// Path the data was read from.  Kept so `Poll` can re-authorize instead of
    /// trusting an open-time decision that a rule change may have invalidated.
    pub path: String,
    /// Pre-read file contents.  Data is available immediately with the current
    /// synchronous VirtIO block backend.
    pub data: Vec<u8>,
}

/// Table of pending reads keyed by opaque handle ID.
///
/// One table serves every client cell, so the keyspace is shared: an entry is
/// reachable only by the cell recorded as its owner.
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

    /// Insert pre-read data on behalf of `owner` and return its handle.
    ///
    /// The handle is usable only by `owner`; no other cell can poll it.
    pub fn insert(&mut self, owner: Caller, path: &str, data: Vec<u8>) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1); // skip 0
        self.slots.insert(
            id,
            PendingRead {
                owner,
                path: path.to_string(),
                data,
            },
        );
        id
    }

    /// The path a slot was filled from, if `caller` owns that slot.
    ///
    /// Lets `Poll` re-check the path rules before handing over data.  A slot
    /// belonging to another cell reads as absent, same as a stale handle.
    pub fn owned_path(&self, caller: Caller, handle: u32) -> Option<&str> {
        self.slots
            .get(&handle)
            .filter(|slot| slot.owner == caller)
            .map(|slot| slot.path.as_str())
    }

    /// Consume the data for `handle` on behalf of `caller`.
    ///
    /// Returns `None` when the handle is stale (already polled), was never
    /// issued, or is owned by a different cell.  Those three cases are
    /// deliberately indistinguishable: a distinguishable "wrong owner" reply
    /// would turn the sequential handle space into an existence oracle.
    ///
    /// A rejected poll leaves the slot untouched.  Removing first and comparing
    /// afterwards would let any cell destroy another cell's pending read by
    /// sweeping the handle space, even while receiving none of the data.
    pub fn poll(&mut self, caller: Caller, handle: u32) -> Option<Vec<u8>> {
        match self.slots.get(&handle) {
            Some(slot) if slot.owner == caller => self.slots.remove(&handle).map(|s| s.data),
            _ => None,
        }
    }
}

impl Default for PendingTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    use types::CellId;

    const CELL_A: Caller = Caller {
        cell: CellId(11),
        generation: 1,
    };
    const CELL_B: Caller = Caller {
        cell: CellId(22),
        generation: 1,
    };
    /// Cell 22 again after a respawn that reused its id: a different principal.
    const CELL_B_RESPAWNED: Caller = Caller {
        cell: CellId(22),
        generation: 2,
    };

    #[test]
    fn poll_returns_data_to_the_issuing_cell() {
        let mut table = PendingTable::new();
        let handle = table.insert(CELL_B, "/data/b", vec![1u8, 2, 3]);
        assert_eq!(table.poll(CELL_B, handle), Some(vec![1u8, 2, 3]));
        // A successful poll consumes the slot.
        assert_eq!(table.poll(CELL_B, handle), None);
    }

    #[test]
    fn poll_rejects_another_cells_handle() {
        let mut table = PendingTable::new();
        let handle = table.insert(CELL_B, "/data/b", vec![0xAA; 8]);
        assert_eq!(table.poll(CELL_A, handle), None);
    }

    #[test]
    fn handle_sweep_neither_reads_nor_destroys_another_cells_slot() {
        let mut table = PendingTable::new();
        let contents = vec![0xBE; 16];
        let handle = table.insert(CELL_B, "/data/b", contents.clone());

        // Cell A sweeps the low handle space the way an attacker would.
        for probe in 1..=64u32 {
            assert_eq!(
                table.poll(CELL_A, probe),
                None,
                "handle {probe} readable by a non-owner"
            );
        }

        // The owner's slot survived the sweep.
        assert_eq!(table.poll(CELL_B, handle), Some(contents));
    }

    #[test]
    fn a_respawned_cell_cannot_poll_its_predecessors_slot() {
        let mut table = PendingTable::new();
        let handle = table.insert(CELL_B, "/data/b", vec![0xCD; 4]);
        assert_eq!(table.poll(CELL_B_RESPAWNED, handle), None);
        // Refused, not consumed.
        assert_eq!(table.poll(CELL_B, handle), Some(vec![0xCD; 4]));
    }

    #[test]
    fn owned_path_is_visible_to_the_issuing_cell_only() {
        let mut table = PendingTable::new();
        let handle = table.insert(CELL_B, "/data/b", vec![1u8]);
        assert_eq!(table.owned_path(CELL_B, handle), Some("/data/b"));
        assert_eq!(table.owned_path(CELL_A, handle), None);
        assert_eq!(table.owned_path(CELL_B_RESPAWNED, handle), None);
    }

    #[test]
    fn poll_rejects_a_handle_that_was_never_issued() {
        let mut table = PendingTable::new();
        let handle = table.insert(CELL_A, "/data/a", vec![7u8]);
        assert_eq!(table.poll(CELL_A, handle.wrapping_add(1)), None);
    }
}
