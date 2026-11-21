//! Pending async read table for the VFS service.
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::caller::Caller;
use crate::namespace::ServiceHandle;

/// Most pending reads one caller generation may hold at once.
pub const MAX_PENDING_PER_CALLER: usize = 8;

/// Most pending reads the table holds across every caller. Each slot may pin a
/// `/srv` service lease, so the bound also caps how many keys reads can hold
/// against unlink, rmdir and rename.
pub const MAX_PENDING: usize = 64;

/// Bytes one `Poll` reply carries: the 512-byte IPC frame less the postcard
/// envelope. A slot never stores more, because nothing past it is deliverable.
pub const POLL_PAYLOAD: usize = 480;

/// One pending async read slot.
pub struct PendingRead {
    pub owner: Caller,
    pub path: String,
    pub data: Vec<u8>,
    pub _lease: Option<ServiceHandle>,
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

    /// Whether `owner` may open one more slot. Checked before the read and
    /// before any lease, so a refused request holds nothing.
    pub fn has_room(&self, owner: Caller) -> bool {
        self.slots.len() < MAX_PENDING
            && self.slots.values().filter(|s| s.owner == owner).count() < MAX_PENDING_PER_CALLER
    }

    /// Insert pre-read data on behalf of `owner` and return its handle; `None`
    /// when [`Self::has_room`] refuses. `data` beyond [`POLL_PAYLOAD`] is dropped.
    pub fn insert(
        &mut self,
        owner: Caller,
        path: &str,
        mut data: Vec<u8>,
        lease: Option<ServiceHandle>,
    ) -> Option<u32> {
        if !self.has_room(owner) {
            return None;
        }
        data.truncate(POLL_PAYLOAD);
        let mut id = self.next_id;
        // Skip 0 and, after the u32 space wraps, any handle still outstanding.
        while id == 0 || self.slots.contains_key(&id) {
            id = id.wrapping_add(1);
        }
        self.next_id = id.wrapping_add(1);
        self.slots.insert(
            id,
            PendingRead {
                owner,
                path: path.to_string(),
                data,
                _lease: lease,
            },
        );
        Some(id)
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

    pub fn purge_owner(&mut self, caller: Caller) -> usize {
        let doomed: alloc::vec::Vec<u32> = self
            .slots
            .iter()
            .filter(|(_, slot)| slot.owner == caller)
            .map(|(handle, _)| *handle)
            .collect();
        let removed = doomed.len();
        for handle in doomed {
            let _ = self.slots.remove(&handle);
        }
        removed
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

    const CELL_A: Caller = Caller::principal(CellId(11), 1);
    const CELL_B: Caller = Caller::principal(CellId(22), 1);
    /// Cell 22 again after a respawn that reused its id: a different principal.
    const CELL_B_RESPAWNED: Caller = Caller::principal(CellId(22), 2);

    #[test]
    fn poll_returns_data_to_the_issuing_cell() {
        let mut table = PendingTable::new();
        let handle = table
            .insert(CELL_B, "/data/b", vec![1u8, 2, 3], None)
            .unwrap();
        assert_eq!(table.poll(CELL_B, handle), Some(vec![1u8, 2, 3]));
        // A successful poll consumes the slot.
        assert_eq!(table.poll(CELL_B, handle), None);
    }

    #[test]
    fn poll_rejects_another_cells_handle() {
        let mut table = PendingTable::new();
        let handle = table
            .insert(CELL_B, "/data/b", vec![0xAA; 8], None)
            .unwrap();
        assert_eq!(table.poll(CELL_A, handle), None);
    }

    #[test]
    fn handle_sweep_neither_reads_nor_destroys_another_cells_slot() {
        let mut table = PendingTable::new();
        let contents = vec![0xBE; 16];
        let handle = table
            .insert(CELL_B, "/data/b", contents.clone(), None)
            .unwrap();

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
        let handle = table
            .insert(CELL_B, "/data/b", vec![0xCD; 4], None)
            .unwrap();
        assert_eq!(table.poll(CELL_B_RESPAWNED, handle), None);
        // Refused, not consumed.
        assert_eq!(table.poll(CELL_B, handle), Some(vec![0xCD; 4]));
    }

    #[test]
    fn owned_path_is_visible_to_the_issuing_cell_only() {
        let mut table = PendingTable::new();
        let handle = table.insert(CELL_B, "/data/b", vec![1u8], None).unwrap();
        assert_eq!(table.owned_path(CELL_B, handle), Some("/data/b"));
        assert_eq!(table.owned_path(CELL_A, handle), None);
        assert_eq!(table.owned_path(CELL_B_RESPAWNED, handle), None);
    }

    #[test]
    fn poll_rejects_a_handle_that_was_never_issued() {
        let mut table = PendingTable::new();
        let handle = table.insert(CELL_A, "/data/a", vec![7u8], None).unwrap();
        assert_eq!(table.poll(CELL_A, handle.wrapping_add(1)), None);
    }

    #[test]
    fn purge_owner_is_exact_to_the_generation() {
        let mut table = PendingTable::new();
        let old = table.insert(CELL_B, "/data/b", vec![1u8], None).unwrap();
        let new = table
            .insert(CELL_B_RESPAWNED, "/data/c", vec![2u8], None)
            .unwrap();
        assert_eq!(table.purge_owner(CELL_B), 1);
        assert_eq!(table.poll(CELL_B, old), None);
        assert_eq!(table.poll(CELL_B_RESPAWNED, new), Some(vec![2u8]));
    }

    #[test]
    fn one_caller_holds_at_most_its_share_and_others_keep_theirs() {
        let mut table = PendingTable::new();
        let handles: Vec<u32> = (0..MAX_PENDING_PER_CALLER)
            .map(|_| table.insert(CELL_A, "/data/a", vec![1u8], None).unwrap())
            .collect();
        assert!(!table.has_room(CELL_A));
        assert_eq!(table.insert(CELL_A, "/data/a", vec![1u8], None), None);
        // Another principal, a respawn of the same cell id included, is unaffected.
        assert!(table.insert(CELL_B, "/data/b", vec![2u8], None).is_some());
        assert!(table
            .insert(CELL_B_RESPAWNED, "/data/b", vec![2u8], None)
            .is_some());
        // Polling frees a slot for its owner.
        assert_eq!(table.poll(CELL_A, handles[0]), Some(vec![1u8]));
        assert!(table.insert(CELL_A, "/data/a", vec![1u8], None).is_some());
    }

    #[test]
    fn the_table_is_bounded_across_callers() {
        let mut table = PendingTable::new();
        for cell in 0..(MAX_PENDING / MAX_PENDING_PER_CALLER) as u64 {
            let owner = Caller::principal(CellId(100 + cell), 1);
            for _ in 0..MAX_PENDING_PER_CALLER {
                assert!(table.insert(owner, "/data/x", vec![0u8], None).is_some());
            }
        }
        let fresh = Caller::principal(CellId(999), 1);
        assert!(!table.has_room(fresh));
        assert_eq!(table.insert(fresh, "/data/x", vec![0u8], None), None);
    }

    #[test]
    fn a_slot_keeps_only_what_one_poll_can_carry() {
        let mut table = PendingTable::new();
        let handle = table
            .insert(CELL_A, "/data/a", vec![7u8; 4 * POLL_PAYLOAD], None)
            .unwrap();
        assert_eq!(table.poll(CELL_A, handle), Some(vec![7u8; POLL_PAYLOAD]));
    }

    #[test]
    fn a_wrapped_handle_counter_skips_outstanding_handles() {
        let mut table = PendingTable::new();
        table.next_id = u32::MAX;
        let last = table.insert(CELL_A, "/data/a", vec![1u8], None).unwrap();
        assert_eq!(last, u32::MAX);
        table.next_id = u32::MAX;
        let next = table.insert(CELL_A, "/data/a", vec![2u8], None).unwrap();
        assert_eq!(next, 1);
        assert_eq!(table.poll(CELL_A, last), Some(vec![1u8]));
    }
}
