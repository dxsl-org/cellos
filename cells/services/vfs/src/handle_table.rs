#![allow(dead_code)] // reason: write path wired in full VirtIO-FAT phase
//! Open file handle table for the VFS service.
//!
//! Maps a `CapId` (issued by the kernel) to the VFS-internal state needed to
//! service subsequent `Read`, `Write`, `Seek`, and `Close` IPC requests.
//!
//! One table serves every client cell — the `CapId` keyspace is shared, not
//! per-cell.  Each entry therefore records the cell that opened it, and every
//! lookup takes the caller's identity and compares it before handing back state.
//! Holding a `CapId` value is not by itself sufficient to reach an entry.

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use api::cap::CapId;
use types::VAddr;

use crate::caller::Caller;

/// State for one open file handle inside the VFS cell.
pub struct HandleEntry {
    /// The caller that opened this handle: the only caller permitted to reach the
    /// entry, and the cell whose quota it is accounted against.  Compared on
    /// every lookup — see [`HandleTable::get_mut`] and [`HandleTable::remove`].
    /// Carries the cell generation, so a successor cell under a recycled
    /// `CellId` is not the same owner.
    pub owner: Caller,
    /// Path this handle was opened for.  Kept so a read through the handle can be
    /// re-authorized against the current rules: the open-time check proves only
    /// what policy said then, and a handle can outlive a rule change.
    pub path: String,
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

    /// Register a new read-only handle for `owner`, backed by `data_ptr/data_len`.
    pub fn insert_ro(
        &mut self,
        owner: Caller,
        cap: CapId,
        path: &str,
        data_ptr: VAddr,
        data_len: usize,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    use types::CellId;

    const CELL_A: Caller = Caller {
        cell: CellId(11),
        generation: 1,
    };
    const CELL_B: Caller = Caller {
        cell: CellId(22),
        generation: 1,
    };
    /// Cell 22 again after a respawn that reused its id.
    const CELL_B_RESPAWNED: Caller = Caller {
        cell: CellId(22),
        generation: 2,
    };
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
}
