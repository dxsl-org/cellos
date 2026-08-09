use crate::caller::Caller;

use super::{DirTable, RevokeOutcome};

impl DirTable {
    /// Remove `roots` and, repeatedly, everything whose parent has gone.
    ///
    /// The fixpoint loop is not an optimisation target: derivation chains are a
    /// handful of entries deep and revocation is rare.
    pub(crate) fn revoke_ids(&mut self, roots: &[u64]) -> RevokeOutcome {
        if roots.is_empty() {
            return RevokeOutcome::none();
        }
        let mut doomed: alloc::vec::Vec<u64> = roots.to_vec();
        let mut cursor = 0;
        while cursor < doomed.len() {
            let id = doomed[cursor];
            cursor += 1;
            let children: alloc::vec::Vec<u64> = self
                .entries
                .iter()
                .filter(|(child, entry)| entry.parent == Some(id) && !doomed.contains(child))
                .map(|(child, _)| *child)
                .collect();
            doomed.extend(children);
        }
        let mut removed = 0;
        for id in &doomed {
            if let Some(entry) = self.entries.remove(id) {
                removed += 1;
                if let Some(state) = self.cells.get_mut(&entry.owner.cell.0) {
                    state.handles = state.handles.saturating_sub(1);
                }
            }
        }
        RevokeOutcome {
            count: removed,
            revoked_ids: doomed,
        }
    }

    /// Revoke everything `cell` holds, at any generation, and forget the cell.
    pub(crate) fn purge_cell(&mut self, cell: u64) -> RevokeOutcome {
        let roots: alloc::vec::Vec<u64> = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.owner.cell.0 == cell)
            .map(|(id, _)| *id)
            .collect();
        let removed = self.revoke_ids(&roots);
        self.cells.remove(&cell);
        removed
    }

    /// Revoke everything held by exactly this caller generation.
    pub(crate) fn purge_owner(&mut self, caller: Caller) -> RevokeOutcome {
        let roots: alloc::vec::Vec<u64> = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.owner == caller)
            .map(|(id, _)| *id)
            .collect();
        let removed = self.revoke_ids(&roots);
        if self.state(caller).is_some() {
            self.cells.remove(&caller.cell.0);
        }
        removed
    }
}
