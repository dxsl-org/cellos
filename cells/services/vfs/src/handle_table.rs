#![allow(dead_code)]
//! Open file handle table for the VFS service.
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use api::cap::CapId;
use types::VAddr;

use crate::caller::Caller;

/// State for one open file handle inside the VFS cell.
pub struct HandleEntry {
    pub owner: Caller,
    pub path: String,
    pub data_ptr: VAddr,
    pub data_len: usize,
    pub pos: usize,
    pub writable: bool,
    pub _lease: Option<crate::namespace::ServiceHandle>,
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

    pub fn insert_ro(&mut self, owner: Caller, cap: CapId, path: &str, ptr: VAddr, len: usize) {
        self.insert_ro_leased(owner, cap, path, ptr, len, None);
    }

    pub fn insert_ro_leased(
        &mut self,
        owner: Caller,
        cap: CapId,
        path: &str,
        data_ptr: VAddr,
        data_len: usize,
        lease: Option<crate::namespace::ServiceHandle>,
    ) {
        self.entries.insert(
            cap.0,
            HandleEntry {
                owner,
                path: path.to_string(),
                data_ptr,
                data_len,
                pos: 0,
                writable: false,
                _lease: lease,
            },
        );
    }

    /// Look up a handle on behalf of `caller`, returning a mutable reference for
    /// read/seek.
    ///
    /// Returns `None` when `cap` is unknown *or* is owned by another cell.  The
    /// two cases are deliberately indistinguishable so that sweeping cap values
    /// cannot be used to discover which handles other cells hold.
    pub fn get_mut(&mut self, caller: Caller, cap: CapId) -> Option<&mut HandleEntry> {
        self.entries.get_mut(&cap.0).filter(|e| e.owner == caller)
    }

    /// The path `cap` was opened for, if `caller` owns it.
    ///
    /// Lets a read through the handle be re-checked against the path rules before
    /// any data moves.  Same indistinguishability as [`Self::get_mut`]: a cap
    /// belonging to another cell reads as unknown.
    pub fn path_of(&self, caller: Caller, cap: CapId) -> Option<&str> {
        self.entries
            .get(&cap.0)
            .filter(|e| e.owner == caller)
            .map(|e| e.path.as_str())
    }

    /// Remove and return a handle owned by `caller` (for Close).
    ///
    /// A caller that does not own `cap` gets `None` and the entry stays in the
    /// table.  Removing first and comparing afterwards would let any cell close
    /// another cell's open file by sweeping cap values.
    pub fn remove(&mut self, caller: Caller, cap: CapId) -> Option<HandleEntry> {
        match self.entries.get(&cap.0) {
            Some(entry) if entry.owner == caller => self.entries.remove(&cap.0),
            _ => None,
        }
    }

    pub fn purge_owner(&mut self, caller: Caller) -> usize {
        let doomed: alloc::vec::Vec<u64> = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.owner == caller)
            .map(|(cap, _)| *cap)
            .collect();
        let removed = doomed.len();
        for cap in doomed {
            let _ = self.entries.remove(&cap);
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use types::CellId;

    const CELL_A: Caller = Caller::principal(CellId(11), 1);
    const CELL_B: Caller = Caller::principal(CellId(22), 1);
    /// Cell 22 again after a respawn that reused its id.
    const CELL_B_RESPAWNED: Caller = Caller::principal(CellId(22), 2);
    const B_CAP: CapId = CapId(7);

    /// Table holding exactly one read-only handle owned by `CELL_B`.
    fn table_owned_by_b() -> HandleTable {
        let mut table = HandleTable::new();
        table.insert_ro(CELL_B, B_CAP, "/data/b", 0x1000, 64);
        table
    }

    #[test]
    fn owner_can_reach_and_close_its_own_handle() {
        let mut table = table_owned_by_b();
        assert!(table.get_mut(CELL_B, B_CAP).is_some());
        assert!(table.remove(CELL_B, B_CAP).is_some());
        assert!(table.get_mut(CELL_B, B_CAP).is_none());
    }

    #[test]
    fn get_mut_rejects_another_cells_handle() {
        let mut table = table_owned_by_b();
        assert!(table.get_mut(CELL_A, B_CAP).is_none());
    }

    #[test]
    fn remove_rejects_another_cells_handle_and_keeps_the_entry() {
        let mut table = table_owned_by_b();
        assert!(table.remove(CELL_A, B_CAP).is_none());
        // The refused remove must not have consumed the owner's handle.
        assert!(table.get_mut(CELL_B, B_CAP).is_some());
    }

    #[test]
    fn path_of_is_visible_to_the_owner_only() {
        let table = table_owned_by_b();
        assert_eq!(table.path_of(CELL_B, B_CAP), Some("/data/b"));
        assert_eq!(table.path_of(CELL_A, B_CAP), None);
        assert_eq!(table.path_of(CELL_B_RESPAWNED, B_CAP), None);
    }

    #[test]
    fn a_respawned_cell_does_not_inherit_its_predecessors_handles() {
        let mut table = table_owned_by_b();
        assert!(table.get_mut(CELL_B_RESPAWNED, B_CAP).is_none());
        assert!(table.remove(CELL_B_RESPAWNED, B_CAP).is_none());
        assert!(table.get_mut(CELL_B, B_CAP).is_some());
    }

    #[test]
    fn cap_sweep_yields_nothing_to_a_non_owner() {
        let mut table = table_owned_by_b();
        for probe in 0..64u64 {
            let cap = CapId(probe);
            assert!(
                table.get_mut(CELL_A, cap).is_none(),
                "cap {probe} readable by a non-owner"
            );
            assert!(
                table.remove(CELL_A, cap).is_none(),
                "cap {probe} removable by a non-owner"
            );
        }
        assert!(table.get_mut(CELL_B, B_CAP).is_some());
    }

    #[test]
    fn purge_owner_is_exact_to_the_generation() {
        let mut table = HandleTable::new();
        table.insert_ro(CELL_B, B_CAP, "/data/b", 0x1000, 64);
        table.insert_ro(CELL_B_RESPAWNED, CapId(8), "/data/c", 0x2000, 32);
        assert_eq!(table.purge_owner(CELL_B), 1);
        assert!(table.get_mut(CELL_B, B_CAP).is_none());
        assert!(table.get_mut(CELL_B_RESPAWNED, CapId(8)).is_some());
    }

    #[test]
    fn handle_table_leased_entry_drops_service_handle_on_remove() {
        let mut table = HandleTable::new();
        let ledger = crate::namespace::NamespaceLedger::new();
        let key = crate::namespace::NamespaceKey::parse("/srv/item").expect("key");
        let lease = ledger.acquire_service_handle(&key).expect("lease");
        assert_eq!(ledger.entry_count(), 1);
        table.insert_ro_leased(CELL_B, B_CAP, "/srv/item", 0x1000, 64, Some(lease));
        assert!(table.remove(CELL_B, B_CAP).is_some());
        assert_eq!(ledger.entry_count(), 0);
    }
}
